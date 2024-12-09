//! rspc-tauri: Tauri integration for [rspc](https://rspc.dev).
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(
    html_logo_url = "https://github.com/specta-rs/rspc/raw/main/.github/logo.png",
    html_favicon_url = "https://github.com/specta-rs/rspc/raw/main/.github/logo.png"
)]

use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use rspc_core::{ProcedureError, Procedures};
use serde::{de::Error, Deserialize, Serialize};
use serde_json::{value::RawValue, Serializer};
use tauri::{
    async_runtime::{spawn, JoinHandle},
    generate_handler,
    ipc::{Channel, InvokeResponseBody, IpcResponse},
    plugin::{Builder, TauriPlugin},
    Manager,
};

struct RpcHandler<R, TCtxFn, TCtx> {
    subscriptions: Mutex<HashMap<u32, JoinHandle<()>>>,
    ctx_fn: TCtxFn,
    procedures: Procedures<TCtx>,
    phantom: std::marker::PhantomData<fn() -> R>,
}

impl<R, TCtxFn, TCtx> RpcHandler<R, TCtxFn, TCtx>
where
    R: tauri::Runtime,
    TCtxFn: Fn(tauri::Window<R>) -> TCtx + Send + Sync + 'static,
    TCtx: Send + 'static,
{
    fn subscriptions(&self) -> MutexGuard<HashMap<u32, JoinHandle<()>>> {
        self.subscriptions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn handle_rpc_impl(
        self: Arc<Self>,
        window: tauri::Window<R>,
        channel: tauri::ipc::Channel<IpcResultResponse>,
        req: Request,
    ) {
        match req {
            Request::Request { path, input } => {
                let id = channel.id();
                let ctx = (self.ctx_fn)(window);

                let Some(procedure) = self.procedures.get(&Cow::Borrowed(&*path)) else {
                    let err = ProcedureError::NotFound;
                    send(&channel, Some((err.status(), &err)));
                    send::<()>(&channel, None);
                    return;
                };

                let mut stream = match input {
                    Some(i) => procedure.exec_with_deserializer(ctx, i),
                    None => procedure.exec_with_deserializer(ctx, serde_json::Value::Null),
                };

                let this = self.clone();
                let handle = spawn(async move {
                    while let Some(value) = stream.next().await {
                        match value {
                            Ok(v) => send(&channel, Some((200, &v))),
                            Err(err) => send(&channel, Some((err.status(), &err))),
                        }
                    }

                    this.subscriptions().remove(&id);
                    send::<()>(&channel, None);
                });

                // if the client uses an existing ID, we will assume the previous subscription is no longer required
                if let Some(old) = self.subscriptions().insert(id, handle) {
                    old.abort();
                }
            }
            Request::Abort(id) => {
                if let Some(h) = self.subscriptions().remove(&id) {
                    h.abort();
                }
            }
        }
    }
}

trait HandleRpc<R: tauri::Runtime>: Send + Sync {
    fn handle_rpc(
        self: Arc<Self>,
        window: tauri::Window<R>,
        channel: tauri::ipc::Channel<IpcResultResponse>,
        req: Request,
    );
}

impl<R, TCtxFn, TCtx> HandleRpc<R> for RpcHandler<R, TCtxFn, TCtx>
where
    R: tauri::Runtime + Send + Sync,
    TCtxFn: Fn(tauri::Window<R>) -> TCtx + Send + Sync + 'static,
    TCtx: Send + 'static,
{
    fn handle_rpc(
        self: Arc<Self>,
        window: tauri::Window<R>,
        channel: tauri::ipc::Channel<IpcResultResponse>,
        req: Request,
    ) {
        Self::handle_rpc_impl(self, window, channel, req);
    }
}

// Tauri commands can't be generic except for their runtime,
// so we need to store + access the handler behind a trait.
// This way handle_rpc_impl has full access to the generics it was instantiated with,
// while State can be stored a) as a singleton (enforced by the type system!) and b) as type erased Tauri state
struct State<R>(Arc<dyn HandleRpc<R>>);

#[tauri::command]
fn handle_rpc<R: tauri::Runtime>(
    state: tauri::State<'_, State<R>>,
    window: tauri::Window<R>,
    channel: tauri::ipc::Channel<IpcResultResponse>,
    req: Request,
) {
    state.0.clone().handle_rpc(window, channel, req);
}

pub fn plugin<R, TCtxFn, TCtx>(
    procedures: impl AsRef<Procedures<TCtx>>,
    ctx_fn: TCtxFn,
) -> TauriPlugin<R>
where
    R: tauri::Runtime + Send + Sync,
    TCtxFn: Fn(tauri::Window<R>) -> TCtx + Send + Sync + 'static,
    TCtx: Send + Sync + 'static,
{
    let procedures = procedures.into();

    Builder::new("rspc")
        .invoke_handler(generate_handler![handle_rpc])
        .setup(move |app_handle, _| {
            if !app_handle.manage(State(Arc::new(RpcHandler {
                subscriptions: Default::default(),
                ctx_fn,
                procedures,
                phantom: Default::default(),
            }))) {
                panic!("Attempted to mount `rspc_tauri::plugin` multiple times. Please ensure you only mount it once!");
            }

            Ok(())
        })
        .build()
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "method", content = "params", rename_all = "camelCase")]
enum Request<'a> {
    /// A request to execute a procedure.
    Request {
        path: String,
        #[serde(borrow)]
        input: Option<&'a RawValue>,
    },
    /// Abort a running task
    /// You must provide the ID of the Tauri channel provided when the task was started.
    Abort(u32),
}

fn send<T: Serialize>(channel: &Channel<IpcResultResponse>, value: Option<(u16, &T)>) {
    #[derive(Serialize)]
    struct Response<'a, T: Serialize> {
        code: u16,
        value: &'a T,
    }

    match value {
        Some((code, value)) => {
            let mut buffer = Vec::with_capacity(128);
            let mut serializer = Serializer::new(&mut buffer);
            channel
                .send(IpcResultResponse(
                    Response { code, value }
                        .serialize(&mut serializer)
                        .map(|_: ()| InvokeResponseBody::Raw(buffer))
                        .map_err(|err| err.to_string()),
                ))
                .ok()
        }
        None => channel
            .send(IpcResultResponse(Ok(InvokeResponseBody::Raw(
                "DONE".into(),
            ))))
            .ok(),
    };
}

#[derive(Clone)]
struct IpcResultResponse(Result<InvokeResponseBody, String>);

impl IpcResponse for IpcResultResponse {
    fn body(self) -> tauri::Result<InvokeResponseBody> {
        self.0.map_err(|err| serde_json::Error::custom(err).into())
    }
}
