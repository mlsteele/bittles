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

use bip_bencode::{BencodeRef, BDecodeOpt};
use docopt::Docopt;
use ring::rand::SystemRandom;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

use errors::*;
use manifest::*;
use metainfo::*;
use peer_protocol::PeerID;

const USAGE: &'static str = "
Usage: bittles <torrent>
";

#[derive(RustcDecodable)]
struct Args {
    arg_torrent: String,
}

fn main() {
    if let Err(ref e) = inner() {
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
            writeln!(stderr, "backtrace: [no backtrace, run with RUST_BACKTRACE=1]").expect(errmsg);
        }

        ::std::process::exit(1);
    }
}

fn inner() -> Result<()> {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.decode())
        .unwrap_or_else(|e| e.exit());

    let cwd = std::env::current_dir().chain_err(|| "get cwd")?;
    println!("cwd: {}", cwd.display());
    println!("torrent: {}", args.arg_torrent);

    let torrent_path = Path::new(&args.arg_torrent);
    let mut buf = vec![0;0];
    let mut f = File::open(torrent_path).chain_err(|| "open torrent file")?;
    f.read_to_end(&mut buf).unwrap();

    let res = BencodeRef::decode(buf.as_slice(), BDecodeOpt::default()).chain_err(|| "decode torrent")?;

    let info = MetaInfo::new(res)?;
    println!("{}", info);

    let manifest = Manifest::new(info.clone());
    println!("{}", manifest);

    let rand = SystemRandom::new();

    let peer_id = PeerID::new(&rand)?;
    println!("peer_id: {:?}", peer_id);

    let datastore_path = {
        let mut x = cwd.clone();
        x.push("tmp");
        x.push("data");
        x
    };
    println!("datastore path: {:?}", datastore_path);

    let manifest_path = {
        let mut x = cwd.clone();
        x.push("tmp");
        x.push("manifest");
        x
    };
    println!("manifest path: {:?}", manifest_path);

    downloader::start(info, peer_id, datastore_path, manifest_path)?;
    Ok(())
}
