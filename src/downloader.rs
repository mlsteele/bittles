use datastore::DataStore;
use error::{Error,Result};
use metainfo::{MetaInfo};
use peer_protocol::{PeerID,Message};
use peer_protocol;
use std::default::Default;
use std::io::Write;
use std::path::Path;
use tracker::{TrackerClient};
use util::{tcp_connect};
use std::time::Duration;
use manifest::{ManifestWithFile,Manifest};
use fillable::*;

pub struct Downloader {
}

impl Downloader {
    pub fn start<P: AsRef<Path>>(info: MetaInfo, peer_id: PeerID, store_path: P, manifest_path: P) -> Result<()> {
        let mut datastore = DataStore::create_or_open(&info, store_path)?;
        let mut manifest = ManifestWithFile::load_or_new(info.clone(), manifest_path)?;
        let mut tc = TrackerClient::new(info.clone(), peer_id.clone())?;

        let tracker_res = tc.easy_start()?;
        println!("tracker res: {:#?}", tracker_res);
        if let Some(reason) = tracker_res.failure_reason {
            return Err(Error::new_str(&format!("tracker failed: {}", reason)));
        }

        if tracker_res.peers.is_empty() {
            return Err(Error::new_str("tracker returned no peers"));
        }

        let peer = tracker_res.peers[0].clone();
        println!("connecting...");
        let mut stream = tcp_connect(peer.address, Duration::from_millis(3000))?;
        println!("connected");
        peer_protocol::handshake_send(&mut stream, info.info_hash.clone(), peer_id.clone())?;
        stream.flush()?;

        let remote_info_hash = peer_protocol::handshake_read_1(&mut stream)?;
        println!("remote info hash: {:?}", remote_info_hash);
        if remote_info_hash != info.info_hash {
            return Err(Error::new_str(&format!("peer info hash mismatch peer:{:?} me:{:?}",
                remote_info_hash, info.info_hash)));
        }
        let remote_peer_id = peer_protocol::handshake_read_2(&mut stream)?;
        println!("remote peer id: {:?}", remote_peer_id);

        maybe_send_bitfield(&mut stream, &manifest.manifest)?;

        let mut state = PeerState::new(info.num_pieces());
        let mut s = BlahState::default();
        loop {
            let m = peer_protocol::read_message(&mut stream)?;
            println!("message: {}", m.summarize());
            match m {
                Message::Choke =>         state.peer_choking = true,
                Message::Unchoke =>       state.peer_choking = false,
                Message::Interested =>    state.peer_interested = true,
                Message::NotInterested => state.peer_interested = false,
                Message::Bitfield { bits } => {
                    if bits.len() < info.num_pieces() {
                        println!("{}", Error::new_str(&format!("bitfield has less bits {} than pieces {}",
                                                           bits.len(), info.num_pieces())));
                    }
                    let mut i_start = 0;
                    let mut in_interval = false;
                    for b in 0..info.num_pieces() {
                        if bits[b] && !in_interval {
                            i_start = b;
                            in_interval = true;
                        } else if !bits[b] && in_interval {
                            state.has.add(i_start as u32, b as u32);
                            in_interval = false;
                        }
                    }
                    if in_interval {
                        state.has.add(i_start as u32, info.num_pieces() as u32);
                    }
                },
                Message::Have { piece } => {
                    state.has.add(piece, piece+1);
                },
                Message::Piece { piece, offset, block } => {
                    datastore.write_block(piece as usize, offset, &block)?;
                    manifest.manifest.add_block(piece as usize, offset, block.len() as u32)?;
                    manifest.store()?;
                    s.waiting = false;
                },
                _ => {},
            }
            s.nreceived += 1;
            if s.nreceived >= 1 && state.am_choking {
                let out = Message::Unchoke{};
                println!("sending message: {:?}", out);
                peer_protocol::send_message(&mut stream, &out)?;
                state.am_choking = false;
            }
            if s.nreceived >= 1 && !state.am_interested {
                let out = Message::Interested{};
                println!("sending message: {:?}", out);
                peer_protocol::send_message(&mut stream, &out)?;
                state.am_interested = true;
            }
            if !s.waiting && !state.peer_choking && state.am_interested {
                match manifest.manifest.next_desired_block() {
                    None => {
                        return Err(Error::todo())
                    },
                    Some(desire) => {
                        let out = Message::Request{
                            piece: desire.piece,
                            offset: desire.offset,
                            length: desire.length,
                        };
                        println!("sending message: {:?}", out);
                        peer_protocol::send_message(&mut stream, &out)?;
                        s.waiting = true;
                    }
                }
            }
            println!("state: {:?}", state);
        }
    }
}

fn maybe_send_bitfield<T: Write>(stream: &mut T, manifest: &Manifest) -> Result<()> {
    // TODO(jessk)
    Ok(())
}

#[derive(Debug)]
pub struct PeerState {
    /// Peer is interested in this client
    peer_interested: bool,
    /// Peer is choking this client
    peer_choking: bool,

    am_interested: bool,
    am_choking: bool,

    has: Fillable,
}

#[derive(Debug, Default)]
pub struct BlahState {
    nreceived: u64,
    requested: bool,
    waiting: bool,
}

impl PeerState {
    fn new(num_pieces: usize) -> Self {
        PeerState {
            peer_interested: false,
            peer_choking: true,
            am_interested: false,
            am_choking: true,
            has: Fillable::new(num_pieces as u32),
        }
    }
}
