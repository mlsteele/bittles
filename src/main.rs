#![allow(dead_code)]

extern crate bip_bencode;
extern crate byteorder;
extern crate docopt;
extern crate hyper;
extern crate itertools;
extern crate ring;
extern crate rustc_serialize;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_cbor;
extern crate url;

mod downloader;
mod datastore;
mod error;
mod metainfo;
mod manifest;
mod tracker;
#[macro_use]
mod util;
mod peer_protocol;
mod fillable;

use bip_bencode::{BencodeRef, BDecodeOpt};
use docopt::Docopt;
use downloader::Downloader;
use itertools::Itertools;
use metainfo::*;
use manifest::*;
use peer_protocol::{PeerID};
use ring::rand::SystemRandom;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use util::{replace_query_parameters, QueryParameters};

const USAGE: &'static str = "
Usage: bittles <torrent>
";

#[derive(RustcDecodable)]
struct Args {
    arg_torrent: String,
}

fn main() {
    match inner() {
        Ok(_) => {}
        Err(e) => println!("ERROR: {}", e)
    }
}

#[allow(dead_code)]
fn inner2() -> Result<(),Box<Error>> {
    let x: [u8; 20] = [18, 52, 86, 120, 154, 188, 222, 241, 35, 69, 103, 137, 171, 205, 239, 18, 52, 86, 120, 154];
    let y = format!("{:x}", x.iter().format(""));
    println!("hex: {}", y);

    let z = url::percent_encoding::percent_encode(&x, url::percent_encoding::QUERY_ENCODE_SET).collect::<String>();
    println!("url encoded: {}", z);

    assert_eq!("%124Vx%9A%BC%DE%F1%23Eg%89%AB%CD%EF%124Vx%9A", z);

    {
        let mut url = hyper::Url::parse("http://example.com/announce")?;
        {
            let mut qp = url.query_pairs_mut();
            qp.clear();
            // qp.append_pair("info_hash", "asdlfkjskd\u{00cc}f");
            qp.append_pair("info_hash", &z);
        }

        println!("url qpm: {:?}", url);
    }

    {
        let mut url = hyper::Url::parse("http://example.com/announce")?;
        url.set_query(Some(&format!("info_hash={}", z)));
        println!("url s-q: {:?}", url);
    }

    {
        let mut url = hyper::Url::parse("http://example.com/announce")?;
        replace_query_parameters(&mut url,
                                 &[("info_hash", x)]
        );
        println!("url uuz: {:?}", url);
    }

    {
        let mut url = hyper::Url::parse("http://example.com/announce")?;
        let mut qps = QueryParameters::new();
        qps.push("info_hash", x);
        qps.apply(&mut url);
        println!("url uub: {:?}", url);
    }

    Ok(())
}

#[allow(dead_code)]
fn inner3() -> Result<(),Box<Error>> {
    // ubuntu info hash
    let x: [u8; 20] = [52, 147, 6, 116, 239, 59, 185, 49, 127, 181, 242, 99, 204, 168, 48, 245, 38, 133, 35, 91];
    let y = format!("{:x}", x.iter().format(""));
    println!("hex: {}", y);

    let reference = "4%93%06t%EF%3B%B91%7F%B5%F2c%CC%A80%F5%26%85%23%5B";

    // let z = url::percent_encoding::percent_encode(&x, url::percent_encoding::QUERY_ENCODE_SET).collect::<String>();
    let z = url::form_urlencoded::byte_serialize(&x).collect::<String>();
    println!("        ref: {}", reference);
    println!("url encoded: {}", z);

    assert_eq!(reference, z);

    {
        let mut url = hyper::Url::parse("http://example.com/announce")?;
        let mut qps = QueryParameters::new();
        qps.push("info_hash", x);
        qps.apply(&mut url);
        println!("url uub: {:?}", url);
    }

    Ok(())
}

fn inner() -> Result<(),Box<Error>> {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.decode())
        .unwrap_or_else(|e| e.exit());

    let cwd = std::env::current_dir()?;
    println!("cwd: {}", cwd.display());
    println!("torrent: {}", args.arg_torrent);

    let torrent_path = Path::new(&args.arg_torrent);
    let mut buf = vec![0;0];
    let mut f = File::open(torrent_path)?;
    f.read_to_end(&mut buf).unwrap();

    let res = BencodeRef::decode(buf.as_slice(), BDecodeOpt::default())?;

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

    Downloader::start(info, peer_id, datastore_path, manifest_path)?;
    Ok(())
}
