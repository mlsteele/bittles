use futures::future::Future;
use std;

mod errors_gen {
    // Create the Error, ErrorKind, ResultExt, and Result types
    error_chain!{
        foreign_links {

            Bencode(::bip_bencode::BencodeParseError);
            Hyper(::hyper::Error);
            Io(::std::io::Error);
            Ring(::ring::error::Unspecified);
            SerdeCbor(::serde_cbor::Error);
            Url(::url::ParseError);

            // Io(io::Error),
            // Utf8(str::Utf8Error),
            // SerdeCbor(serde_cbor::Error),
            // MpscSendError(Box<std::error::Error>),
            // MpscRecvError(mpsc::RecvError),

        }
    }
}

pub use self::errors_gen::{Error, ErrorKind, Result, ResultExt};

// https://github.com/brson/error-chain/issues/90
type SFuture<T> = Box<Future<Item = T, Error = Error>>;

pub trait FutureChainErr<T> {
    fn chain_err<F, E>(self, callback: F) -> SFuture<T>
        where F: FnOnce() -> E + 'static,
              E: Into<ErrorKind>;
}

impl<F> FutureChainErr<F::Item> for F
    where F: Future + 'static,
          F::Error: std::error::Error + Send + 'static
{
    fn chain_err<C, E>(self, callback: C) -> SFuture<F::Item>
        where C: FnOnce() -> E + 'static,
              E: Into<ErrorKind>
    {
        Box::new(self.then(|r| r.chain_err(callback)))
    }
}
