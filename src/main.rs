#![allow(dead_code)]

// `error_chain!` can recurse deeply
#![recursion_limit = "1024"]

extern crate bip_bencode;
extern crate byteorder;
extern crate docopt;
#[macro_use]
extern crate error_chain;
extern crate hyper;
extern crate itertools;
extern crate ring;
extern crate rustc_serialize;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_cbor;
#[macro_use]
extern crate slog;
extern crate slog_term;
extern crate slog_async;
extern crate url;
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;
extern crate bytes;

mod downloader;
mod datastore;
mod errors;
mod metainfo;
mod manifest;
mod tracker;
#[macro_use]
mod util;
mod peer_protocol;
mod fillable;

use bip_bencode::{BDecodeOpt, BencodeRef};
use docopt::Docopt;
use errors::*;
use manifest::*;
use metainfo::*;
use peer_protocol::PeerID;
use ring::rand::SystemRandom;
use slog::Logger;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

const USAGE: &'static str = "
Usage: bittles <torrent>
";

#[derive(RustcDecodable)]
struct Args {
    arg_torrent: String,
}

fn main() {

    // Set up loggin
    let decorator = slog_term::TermDecorator::new().build();
    let drain = {
        use slog::Drain;
        let drain = slog_term::FullFormat::new(decorator).build().fuse();
        slog_async::Async::new(drain).build().fuse()
    };
    let log = slog::Logger::root(drain, o!());

    info!(log, "startup");

    if let Err(ref e) = inner(log) {
        use std::io::Write;
        let stderr = &mut ::std::io::stderr();
        let errmsg = "Error writing to stderr";

        writeln!(stderr, "error: {}", e).expect(errmsg);

        for e in e.iter().skip(1) {
            writeln!(stderr, "caused by: {}", e).expect(errmsg);
        }

        // The backtrace is not always generated.
        // Run with `RUST_BACKTRACE=1`
        if let Some(backtrace) = e.backtrace() {
            writeln!(stderr, "backtrace: {:?}", backtrace).expect(errmsg);
        } else {
            writeln!(stderr,
                     "backtrace: [no backtrace, run with RUST_BACKTRACE=1]")
                    .expect(errmsg);
        }

        ::std::process::exit(1);
    }
}

fn inner(log: Logger) -> Result<()> {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.decode())
        .unwrap_or_else(|e| e.exit());

    let cwd = std::env::current_dir().chain_err(|| "get cwd")?;
    info!(log, "cwd: {}", cwd.display());
    info!(log, "torrent: {}", args.arg_torrent);

    let torrent_path = Path::new(&args.arg_torrent);
    let mut buf = vec![0;0];
    let mut f = File::open(torrent_path).chain_err(|| "open torrent file")?;
    f.read_to_end(&mut buf).unwrap();

    let res = BencodeRef::decode(buf.as_slice(), BDecodeOpt::default())
        .chain_err(|| "decode torrent")?;

    let info = MetaInfo::new(res)?;
    info!(log, "{}", info);

    let manifest = Manifest::new(info.clone());
    info!(log, "{}", manifest);

    let rand = SystemRandom::new();

    let peer_id = PeerID::new(&rand)?;
    info!(log, "peer_id: {:?}", peer_id);

    let datastore_path = {
        let mut x = cwd.clone();
        x.push("tmp");
        x.push("data");
        x
    };
    info!(log, "datastore path: {:?}", datastore_path);

    let manifest_path = {
        let mut x = cwd.clone();
        x.push("tmp");
        x.push("manifest");
        x
    };
    info!(log, "manifest path: {:?}", manifest_path);

    downloader::start(log, info, peer_id, datastore_path, manifest_path)?;
    Ok(())
}
