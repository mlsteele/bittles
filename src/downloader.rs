use error::{Error,Result};
use peer::{PeerID,Message};
use peer;
use metainfo::{MetaInfo};
use tracker::{TrackerClient};
use std::net::TcpStream;
use std::io::Write;
use std::default::Default;

pub struct Downloader {
}

impl Downloader {
    pub fn start(info: MetaInfo, peer_id: PeerID) -> Result<()> {
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
        let mut stream = TcpStream::connect(peer.address)?;
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
        let mut nreceived = 0;
        let mut requested = false;
        loop {
            let m = peer::read_message(&mut stream)?;
            println!("message: {}", m.summarize());
            match m {
                Message::Choke =>         state.peer_choking = true,
                Message::Unchoke =>       state.peer_choking = false,
                Message::Interested =>    state.peer_interested = true,
                Message::NotInterested => state.peer_interested = false,
                Message::Bitfield { bits } => {
                    if (bits.len() as u64) < info.num_pieces() {
                        println!("{}", Error::new_str(&format!("bitfield has less bits {} than pieces {}",
                                                           bits.len(), info.num_pieces())));
                    }
                },
                _ => {},
            }
            nreceived += 1;
            if nreceived >= 1 && state.am_choking {
                let out = Message::Unchoke{};
                println!("sending message: {:?}", out);
                peer::send_message(&mut stream, &out)?;
                state.am_choking = false;
            }
            if nreceived >= 1 && !state.am_interested {
                let out = Message::Interested{};
                println!("sending message: {:?}", out);
                peer::send_message(&mut stream, &out)?;
                state.am_interested = true;
            }
            if !requested && !state.peer_choking && state.am_interested {
                let out = Message::Request{
                    piece: 0,
                    offset: 0,
                    length: 1 << 14,
                };
                println!("sending message: {:?}", out);
                peer::send_message(&mut stream, &out)?;
                requested = true;
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
