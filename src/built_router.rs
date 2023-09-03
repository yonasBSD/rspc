use std::{
    borrow::Cow,
    collections::BTreeMap,
    fs::{self, File},
    io::Write,
    path::PathBuf,
    sync::Arc,
};

use specta::{
    ts::{self, FormatterFn, TsExportError},
    TypeMap,
};

use crate::{
    internal::procedure::{ProcedureStore, ProcedureTodo, ProceduresDef},
    ExportError,
};

/// ExportConfig is used to configure how rspc will export your types.
pub struct ExportConfig {
    export_path: PathBuf,
    header: Cow<'static, str>,
    formatter: Option<FormatterFn>,
}

impl ExportConfig {
    pub fn new(export_path: impl Into<PathBuf>) -> ExportConfig {
        ExportConfig {
            export_path: export_path.into(),
            header: Cow::Borrowed(""),
            formatter: None,
        }
    }

    pub fn header(self, header: impl Into<Cow<'static, str>>) -> Self {
        Self {
            header: header.into(),
            ..self
        }
    }

    pub fn formatter(self, formatter: FormatterFn) -> Self {
        Self {
            formatter: Some(formatter),
            ..self
        }
    }
}

/// BuiltRouter is a router that has been constructed and validated. It is ready to be attached to an integration to serve it to the outside world!
pub struct BuiltRouter<TCtx = ()> {
    pub(crate) queries: ProcedureStore<TCtx>,
    pub(crate) mutations: ProcedureStore<TCtx>,
    pub(crate) subscriptions: ProcedureStore<TCtx>,
    pub(crate) typ_store: TypeMap,
}

impl<TCtx> BuiltRouter<TCtx>
where
    TCtx: Send + 'static,
{
    pub fn arced(self) -> Arc<Self> {
        Arc::new(self)
    }

    #[cfg(feature = "unstable")]
    pub fn typ_store(&self) -> TypeMap {
        self.typ_store.clone()
    }

    #[cfg(not(feature = "unstable"))]
    pub(crate) fn typ_store(&self) -> TypeMap {
        self.typ_store.clone()
    }

    #[cfg(feature = "unstable")]
    pub fn queries(&self) -> &BTreeMap<String, ProcedureTodo<TCtx>> {
        &self.queries.store
    }

    #[cfg(feature = "unstable")]
    pub fn mutations(&self) -> &BTreeMap<String, ProcedureTodo<TCtx>> {
        &self.mutations.store
    }

    #[cfg(feature = "unstable")]
    pub fn subscriptions(&self) -> &BTreeMap<String, ProcedureTodo<TCtx>> {
        &self.subscriptions.store
    }

    #[allow(clippy::panic_in_result_fn)] // TODO: Error handling given we return `Result`
    #[cfg(feature = "typescript")]
    pub fn export_ts(&self, cfg: ExportConfig) -> Result<(), ExportError> {
        if let Some(export_dir) = cfg.export_path.parent() {
            fs::create_dir_all(export_dir)?;
        }
        let mut file = File::create(&cfg.export_path)?;
        if cfg.header != "" {
            writeln!(file, "{}", cfg.header)?;
        }
        writeln!(file, "// This file was generated by [rspc](https://github.com/oscartbeaumont/rspc). Do not edit this file manually.")?;

        let config = ts::ExportConfig::new().bigint(
            ts::BigIntExportBehavior::FailWithReason(
                "rspc does not support exporting bigint types (i64, u64, i128, u128) because they are lossily decoded by `JSON.parse` on the frontend. Tracking issue: https://github.com/oscartbeaumont/rspc/issues/93",
            )
        );

        // TODO: Specta API + `ExportConfig` option for a formatter
        writeln!(
            file,
            "{}",
            ts::export_named_datatype(
                &config,
                &ProceduresDef::new(
                    self.queries.store.values(),
                    self.mutations.store.values(),
                    self.subscriptions.store.values()
                )
                .to_named(),
                &self.typ_store()
            )?
        )?;

        // We sort by name to detect duplicate types BUT also to ensure the output is deterministic. The SID can change between builds so is not suitable for this.
        let types = self
            .typ_store
            .clone()
            .into_iter()
            .filter(|(_, v)| match v {
                Some(_) => true,
                None => {
                    unreachable!(
                        "Placeholder type should never be returned from the Specta functions!"
                    )
                }
            })
            .collect::<BTreeMap<_, _>>();

        // This is a clone of `detect_duplicate_type_names` but using a `BTreeMap` for deterministic ordering
        let mut map = BTreeMap::new();
        for (sid, dt) in &types {
            match dt {
                Some(dt) => {
                    if let Some(ext) = dt.ext() {
                        if let Some((existing_sid, existing_impl_location)) =
                            map.insert(dt.name(), (sid, ext.impl_location().clone()))
                        {
                            if existing_sid != sid {
                                return Err(ExportError::TsExportErr(
                                    TsExportError::DuplicateTypeName(
                                        dt.name().clone(),
                                        ext.impl_location().clone(),
                                        existing_impl_location,
                                    ),
                                ));
                            }
                        }
                    }
                }
                None => unreachable!(),
            }
        }

        for (_, (sid, _)) in map {
            writeln!(
                file,
                "\n{}",
                ts::export_named_datatype(
                    &config,
                    match types.get(sid) {
                        Some(Some(v)) => v,
                        _ => unreachable!(),
                    },
                    &types
                )?
            )?;
        }

        file.flush()?;
        drop(file);

        if let Some(formatter) = cfg.formatter {
            (formatter)(cfg.export_path)?;
        }

        Ok(())
    }
}
