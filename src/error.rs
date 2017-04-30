use std::error;
use std::fmt;
use std::result;
use std::str;
use serde_cbor;

use bip_bencode;
use hyper;
use std::io;
use ring;
use url;

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Generic(String),
    Annotated(String, Box<Error>),
    PeerProtocol(String),
    Ring(ring::error::Unspecified),
    Url(url::ParseError),
    Hyper(hyper::Error),
    Io(io::Error),
    Bencode(bip_bencode::BencodeParseError),
    Utf8(str::Utf8Error),
    SerdeCbor(serde_cbor::Error),
}

impl Error {
    pub fn new_str(description: &str) -> Error {
        Error::Generic(description.to_owned())
    }

    pub fn new_peer(description: &str) -> Error {
        Error::PeerProtocol(description.to_owned())
    }

    pub fn annotate<E>(err: E, description: &str) -> Error
        where E: Into<Error>,
    {
        let err2: Error = err.into();
        let description2: String = format!("{}: {}", description, error::Error::description(&err2));
        Error::Annotated(description2, Box::new(err2))
    }

    #[allow(dead_code)]
    pub fn todo() -> Error {
        Error::new_str("not implemented")
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use std::error::Error;
        write!(f, "{}", self.description())
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match self {
            &Error::Generic(ref description) => description,
            &Error::Annotated(ref description, _) => description,
            &Error::PeerProtocol(ref description) => description,
            &Error::Ring(ref error) => error.description(),
            &Error::Url(ref error) => error.description(),
            &Error::Hyper(ref error) => error.description(),
            &Error::Io(ref error) => error.description(),
            &Error::Bencode(ref error) => error.description(),
            &Error::Utf8(ref error) => error.description(),
            &Error::SerdeCbor(ref error) => error.description(),
        }
    }
}

impl From<ring::error::Unspecified> for Error {
    fn from(err: ring::error::Unspecified) -> Error {
        Error::Ring(err)
    }
}

impl From<url::ParseError> for Error {
    fn from(err: url::ParseError) -> Error {
        Error::Url(err)
    }
}

impl From<hyper::Error> for Error {
    fn from(err: hyper::Error) -> Error {
        Error::Hyper(err)
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::Io(err)
    }
}

impl From<bip_bencode::BencodeParseError> for Error {
    fn from(err: bip_bencode::BencodeParseError) -> Error {
        Error::Bencode(err)
    }
}

impl From<str::Utf8Error> for Error {
    fn from(err: str::Utf8Error) -> Error {
        Error::Utf8(err)
    }
}

impl From<serde_cbor::Error> for Error {
    fn from(err: serde_cbor::Error) -> Error {
        Error::SerdeCbor(err)
    }
}
