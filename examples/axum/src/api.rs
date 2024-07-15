use std::{error, marker::PhantomData, path::PathBuf};

use rspc::{
    procedure::{Procedure, ProcedureBuilder, ResolverInput, ResolverOutput},
    Infallible,
};
use specta_typescript::Typescript;
use specta_util::TypeCollection;
use thiserror::Error;

mod chat;

#[derive(Debug, Error)]
pub enum Error {}

// `Clone` is only required for usage with Websockets
#[derive(Default, Clone)]
pub struct Context {
    pub chat: chat::Ctx,
}

pub type Router = rspc::Router<Context>;

pub struct BaseProcedure<TErr = Error>(PhantomData<TErr>);

impl<TErr> BaseProcedure<TErr> {
    pub fn builder<TInput, TResult>() -> ProcedureBuilder<TErr, Context, Context, TInput, TResult>
    where
        TErr: error::Error + Send + 'static,
        TInput: ResolverInput,
        TResult: ResolverOutput<TErr>,
    {
        Procedure::builder() // You add default middleware here
    }
}

pub fn mount() -> Router {
    Router::new()
        .procedure("version", {
            <BaseProcedure>::builder().query(|_, _: ()| async { Ok(env!("CARGO_PKG_VERSION")) })
        })
        .merge("chat", chat::mount())
        // TODO: I dislike this API
        .ext({
            let mut types = TypeCollection::default();
            types.register::<Infallible>();
            types
        })
        .export_to(
            Typescript::default(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("./bindings.ts"),
        )
}

#[cfg(test)]
#[test]
fn export_rspc_router() {
    mount().build().unwrap();
}