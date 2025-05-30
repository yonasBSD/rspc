//! A procedure holds a single operation that can be executed by the server.
//!
//! A procedure is built up from:
//!  - any number of middleware
//!  - a single resolver function (of type `query`, `mutation` or `subscription`)
//!
//! Features:
//!  - Input types (Serde-compatible or custom)
//!  - Result types (Serde-compatible or custom)
//!  - [`Future`](#todo) or [`Stream`](#todo) results
//!  - Typesafe error handling
//!
//! TODO: Request flow overview
//! TODO: Explain, what a procedure is, return type/struct, middleware, execution order, etc
//!

mod builder;
mod erased;
mod meta;
mod resolver_input;
mod resolver_output;

pub use builder::ProcedureBuilder;
pub use erased::ErasedProcedure;
pub use meta::ProcedureMeta;
pub use resolver_input::ResolverInput;
pub use resolver_output::ResolverOutput;

use std::{borrow::Cow, marker::PhantomData, panic::Location, sync::Arc};

use futures_util::{FutureExt, TryStreamExt};

use specta::{datatype::DataType, Generics, Type};

use crate::{Error, Extension, ProcedureKind, State};

#[derive(Clone)]
pub(crate) struct ProcedureType {
    pub(crate) kind: ProcedureKind,
    pub(crate) input: DataType,
    pub(crate) output: DataType,
    pub(crate) error: DataType,
    pub(crate) location: Location<'static>,
}

/// Represents a single operations on the server that can be executed.
///
/// A [`Procedure`] is built from a [`ProcedureBuilder`] and holds the type information along with the logic to execute the operation.
///
pub struct Procedure<TCtx, TInput, TResult> {
    pub(crate) build:
        Box<dyn FnOnce(Vec<Box<dyn FnOnce(&mut State, ProcedureMeta)>>) -> ErasedProcedure<TCtx>>,
    pub(crate) phantom: PhantomData<(TInput, TResult)>,
}

// TODO: `Debug`, `PartialEq`, `Eq`, `Hash`

impl<TCtx, TInput, TOutput> Procedure<TCtx, TInput, TOutput> {
    /// Construct a new procedure using [`ProcedureBuilder`].
    #[track_caller]
    pub fn builder<TError>(
    ) -> ProcedureBuilder<TError, TCtx, TCtx, TInput, TInput, TOutput, TOutput>
    where
        TCtx: Send + 'static,
        TError: Error,
        // Only the first layer (middleware or the procedure) needs to be a valid input/output type
        TInput: ResolverInput,
        TOutput: ResolverOutput<TError>,
    {
        let location = Location::caller().clone();
        ProcedureBuilder {
            build: Box::new(move |kind, setup, handler| {
                ErasedProcedure {
                    kind,
                    setup: setup
                        .into_iter()
                        .map(|setup| {
                            let v: Box<dyn FnOnce(&mut State)> =
                                Box::new(move |state: &mut State| {
                                    let key: Cow<'static, str> = "todo".to_string().into(); // TODO: Work this out properly
                                    let meta = ProcedureMeta::new(
                                        key.into(),
                                        kind,
                                        Arc::new(State::default()), // TODO: Can we configure a panic instead of this!
                                    );
                                    setup(state, meta);
                                });
                            v
                        })
                        .collect::<Vec<_>>(),
                    location,
                    inner: Box::new(move |state, types| {
                        let key: Cow<'static, str> = "todo".to_string().into(); // TODO: Work this out properly
                        let meta = ProcedureMeta::new(key.clone(), kind, state);

                        (
                            rspc_procedure::Procedure::new(move |ctx, input| {
                                TOutput::into_procedure_stream(
                                    handler(
                                        ctx,
                                        TInput::from_input(input).unwrap(), // TODO: Error handling
                                        meta.clone(),
                                    )
                                    .into_stream()
                                    .map_ok(|v| v.into_stream())
                                    .map_err(|err| err.into_procedure_error())
                                    .try_flatten()
                                    .into_stream(),
                                )
                            }),
                            ProcedureType {
                                kind,
                                location,
                                input: TInput::data_type(types),
                                output: TOutput::data_type(types),
                                error: <TError as Type>::reference(types, &[]).inner,
                            },
                        )
                    }),
                }
            }),
            phantom: PhantomData,
        }
    }

    pub fn with(self, mw: Extension<TCtx, TInput, TOutput>) -> Self
    where
        TCtx: 'static,
    {
        Procedure {
            build: Box::new(move |mut setups| {
                if let Some(setup) = mw.setup {
                    setups.push(setup);
                }
                (self.build)(setups)
            }),
            phantom: PhantomData,
        }
    }

    // TODO: Expose all fields

    // TODO: Make `pub`
    // pub(crate) fn kind(&self) -> ProcedureKind2 {
    //     self.kind
    // }

    // /// Export the [Specta](https://docs.rs/specta) types for this procedure.
    // ///
    // /// TODO - Use this with `rspc::typescript`
    // ///
    // /// # Usage
    // ///
    // /// ```rust
    // /// todo!(); # TODO: Example
    // /// ```
    // pub fn ty(&self) -> &ProcedureTypeDefinition {
    //     &self.ty
    // }

    // /// Execute a procedure with the given context and input.
    // ///
    // /// This will return a [`ProcedureStream`] which can be used to stream the result of the procedure.
    // ///
    // /// # Usage
    // ///
    // /// ```rust
    // /// use serde_json::Value;
    // ///
    // /// fn run_procedure(procedure: Procedure) -> Vec<Value> {
    // ///     procedure
    // ///         .exec((), Value::Null)
    // ///         .collect::<Vec<_>>()
    // ///         .await
    // ///         .into_iter()
    // ///         .map(|result| result.serialize(serde_json::value::Serializer).unwrap())
    // ///         .collect::<Vec<_>>()
    // /// }
    // /// ```
    // pub fn exec<'de, T: ProcedureInput<'de>>(
    //     &self,
    //     ctx: TCtx,
    //     input: T,
    // ) -> Result<ProcedureStream, InternalError> {
    //     match input.into_deserializer() {
    //         Ok(deserializer) => {
    //             let mut input = <dyn erased_serde::Deserializer>::erase(deserializer);
    //             (self.handler)(ctx, &mut input)
    //         }
    //         Err(input) => (self.handler)(ctx, &mut AnyInput(Some(input.into_value()))),
    //     }
    // }
}

impl<TCtx, TInput, TResult> Into<ErasedProcedure<TCtx>> for Procedure<TCtx, TInput, TResult> {
    fn into(self) -> ErasedProcedure<TCtx> {
        (self.build)(Default::default())
    }
}
