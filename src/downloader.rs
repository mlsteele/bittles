use datastore::DataStore;
use error::{Error,Result};
use metainfo::{MetaInfo};
use peer::{PeerID,Message};
use peer;
use std::default::Default;
use std::io::Write;
use std::path::Path;
use tracker::{TrackerClient};
use util::{tcp_connect};
use std::time::Duration;

pub struct Downloader {
}

impl Downloader {
    pub fn start<P: AsRef<Path>>(info: MetaInfo, peer_id: PeerID, store_path: P) -> Result<()> {
        let mut datastore = DataStore::create_or_open(&info, store_path)?;
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
        peer::handshake_send(&mut stream, info.info_hash.clone(), peer_id.clone())?;
        stream.flush()?;

        let remote_info_hash = peer::handshake_read_1(&mut stream)?;
        println!("remote info hash: {:?}", remote_info_hash);
        if remote_info_hash != info.info_hash {
            return Err(Error::new_str(&format!("peer info hash mismatch peer:{:?} me:{:?}",
                remote_info_hash, info.info_hash)));
        }
        let remote_peer_id = peer::handshake_read_2(&mut stream)?;
        println!("remote peer id: {:?}", remote_peer_id);

        let mut state = PeerState::default();
        let mut s = BlahState::default();
        loop {
            let m = peer::read_message(&mut stream)?;
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
                },
                Message::Piece { piece, offset, block } => {
                    datastore.write_block(piece as usize, offset, &block)?;
                    s.waiting = false;
                },
                _ => {},
            }
            s.nreceived += 1;
            if s.nreceived >= 1 && state.am_choking {
                let out = Message::Unchoke{};
                println!("sending message: {:?}", out);
                peer::send_message(&mut stream, &out)?;
                state.am_choking = false;
            }
            if s.nreceived >= 1 && !state.am_interested {
                let out = Message::Interested{};
                println!("sending message: {:?}", out);
                peer::send_message(&mut stream, &out)?;
                state.am_interested = true;
            }
            if !s.waiting && !state.peer_choking && state.am_interested {
                let out = Message::Request{
                    piece: s.piece as u32,
                    offset: 0,
                    length: 1 << 14,
                };
                println!("sending message: {:?}", out);
                peer::send_message(&mut stream, &out)?;
                s.waiting = true;
                s.piece += 1;
            }
            println!("state: {:?}", state);
        }
    }
}

#[derive(Debug)]
pub struct PeerState {
    /// Peer is interested in this client
    peer_interested: bool,
    /// Peer is choking this client
    peer_choking: bool,

    am_interested: bool,
    am_choking: bool,
}

#[derive(Debug, Default)]
pub struct BlahState {
    nreceived: u64,
    requested: bool,
    piece: u64,
    waiting: bool,
}

impl Default for PeerState {
    fn default() -> Self {
        PeerState {
            peer_interested: false,
            peer_choking: true,
            am_interested: false,
            am_choking: true,
        }
    }
}
