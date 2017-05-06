use futures::{Stream,Sink};
use futures::future::{Future,FutureResult};
use futures::future;
use std::default::Default;
use std::net::{SocketAddr};
use std::path::Path;
use std::time::Duration;
use tokio_core::net::{TcpStream};
use tokio_core::reactor;
use tokio_io::codec::{Framed};
use tokio_io::{AsyncRead};

use datastore::DataStore;
use error::{Error,Result};
use fillable::*;
use manifest::{ManifestWithFile};
use metainfo::{MetaInfo,InfoHash};
use peer_protocol::{PeerID,Message,BitTorrentPeerCodec};
use peer_protocol;
use tracker::{TrackerClient};
use util::{tcp_connect2};

type BxFuture<T,E> = Box<Future<Item=T, Error=E>>;

trait FutureEnhanced<T,E> {
    fn bxed(self) -> BxFuture<T,E>
        where Self: Sized + 'static;
}

impl<T,E,X> FutureEnhanced<T,E> for X
    where X: future::Future<Item=T, Error=E> + 'static
{
    fn bxed(self) -> BxFuture<T,E> {
        return Box::new(self)
    }
}

type PeerStream = Framed<TcpStream, BitTorrentPeerCodec>;

struct DownloaderState {
    info: MetaInfo,
    datastore: DataStore,
    manifest: ManifestWithFile,
    peer_state: PeerState,
    blah_state: BlahState,
}

type LoopState = (PeerStream, DownloaderState);

pub fn start<P: AsRef<Path>>(
    info: MetaInfo,
    peer_id: PeerID,
    store_path: P,
    manifest_path: P)
    -> Result<()>
{
    let mut core = reactor::Core::new()?;
    let handle = core.handle();

    let datastore = DataStore::create_or_open(&info, store_path)?;
    let manifest = ManifestWithFile::load_or_new(info.clone(), manifest_path)?;
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

    let num_pieces = info.num_pieces();
    let info_hash = info.info_hash.clone();

    let dstate = DownloaderState {
        info: info,
        datastore: datastore,
        manifest: manifest,
        peer_state: PeerState::new(num_pieces),
        blah_state: BlahState::default(),
    };

    let future_root = connect_peer(peer.address, info_hash, peer_id, &handle)
        .and_then(|(stream, remote_peer_id)| {
            let dstate = dstate;

            main_loop(stream, dstate)

                // .map_err(|err, _s| err)
                // .map(|(_, _s)| Ok(()))

            // Below lies ye ol' blocking implementation
            // for a single stream.

            // let mut state = PeerState::new(info.num_pieces());
            // let mut s = BlahState::default();

            // loop {
            //     let m = peer_protocol::read_message(&mut stream)?;
            //     println!("message: {}", m.summarize());
            //     match m {
            //         Message::Choke =>         state.peer_choking = true,
            //         Message::Unchoke =>       state.peer_choking = false,
            //         Message::Interested =>    state.peer_interested = true,
            //         Message::NotInterested => state.peer_interested = false,
            //         Message::Bitfield { bits } => {
            //             if bits.len() < info.num_pieces() {
            //                 println!("{}", Error::new_str(&format!("bitfield has less bits {} than pieces {}",
            //                                                 bits.len(), info.num_pieces())));
            //             }
            //             let mut i_start = 0;
            //             let mut in_interval = false;
            //             for b in 0..info.num_pieces() {
            //                 if bits[b] && !in_interval {
            //                     i_start = b;
            //                     in_interval = true;
            //                 } else if !bits[b] && in_interval {
            //                     state.has.add(i_start as u32, b as u32);
            //                     in_interval = false;
            //                 }
            //             }
            //             if in_interval {
            //                 state.has.add(i_start as u32, info.num_pieces() as u32);
            //             }
            //         },
            //         Message::Have { piece } => {
            //             state.has.add(piece, piece+1);
            //         },
            //         Message::Piece { piece, offset, block } => {
            //             datastore.write_block(piece as usize, offset, &block)?;
            //             manifest.manifest.add_block(piece as usize, offset, block.len() as u32)?;
            //             manifest.store()?;
            //             s.waiting = false;
            //         },
            //         _ => {},
            //     }
            //     s.nreceived += 1;
            //     if s.nreceived >= 1 && state.am_choking {
            //         let out = Message::Unchoke{};
            //         println!("sending message: {:?}", out);
            //         peer_protocol::send_message(&mut stream, &out)?;
            //         state.am_choking = false;
            //     }
            //     if s.nreceived >= 1 && !state.am_interested {
            //         let out = Message::Interested{};
            //         println!("sending message: {:?}", out);
            //         peer_protocol::send_message(&mut stream, &out)?;
            //         state.am_interested = true;
            //     }
            //     if !s.waiting && !state.peer_choking && state.am_interested {
            //         match manifest.manifest.next_desired_block() {
            //             None => {
            //                 return Err(Error::todo())
            //             },
            //             Some(desire) => {
            //                 let out = Message::Request{
            //                     piece: desire.piece,
            //                     offset: desire.offset,
            //                     length: desire.length,
            //                 };
            //                 println!("sending message: {:?}", out);
            //                 peer_protocol::send_message(&mut stream, &out)?;
            //                 s.waiting = true;
            //             }
            //         }
            //     }
            //     println!("state: {:?}", state);
            // }
        });
    core.run(future_root)?;
    Ok(())
}

/// Connect to a remote peer
fn connect_peer(
    addr: SocketAddr,
    info_hash: InfoHash,
    peer_id: PeerID,
    handle: &reactor::Handle)
    -> BxFuture<(PeerStream, PeerID), Error>
{
    let info_hash2 = info_hash.clone();

    println!("connecting...");
    tcp_connect2(&addr, Duration::from_millis(3000), &handle)
        .map_err(|e| Error::annotate(e, "peer connection failed"))
        .and_then(|stream| -> FutureResult<TcpStream,Error> {
            match stream {
                Some(stream) => future::ok(stream),
                None => future::err(Error::new_str(&"peer connection timed out")),
            }
        })
        .and_then(move |stream| {
            println!("connected");
            peer_protocol::handshake_send_async(stream, info_hash.clone(), peer_id.clone())
        })
        .and_then(|stream| {
            peer_protocol::handshake_read_1_async(stream)
        })
        .and_then(move |(stream, remote_info_hash)| {
            println!("remote info hash: {:?}", remote_info_hash);
            if remote_info_hash != info_hash2 {
                return Err(Error::new_str(&format!("peer info hash mismatch peer:{:?} me:{:?}",
                    remote_info_hash, info_hash2)));
            }
            Ok(stream)
        })
        .and_then(|stream| {
            peer_protocol::handshake_read_2_async(stream)
                .map(move |(stream, remote_peer_id)| {
                    (stream, remote_peer_id)
                })
        })
        .and_then(|(stream, remote_peer_id)| {
            println!("remote peer id: {:?}", remote_peer_id);

            // let stream: Framed<TcpStream,BitTorrentPeerCodec> = stream.framed(BitTorrentPeerCodec);
            let stream: PeerStream = stream.framed(BitTorrentPeerCodec);
            Ok((stream, remote_peer_id))
        })
        .bxed()
}

/// Compose the main loop.
fn main_loop(stream: PeerStream, dstate: DownloaderState) -> BxFuture<(), Error> {
    let lstate: LoopState = (stream, dstate);
    future::loop_fn(lstate, loop_step).bxed()
}

// /// One step of the main loop.
// fn loop_step(lstate: LoopState) -> BxFuture<future::Loop<(), LoopState>, Error> {
//     use futures::future::Loop::{Break,Continue};
//     let (stream, mut dstate) = lstate;
//     stream
//         .into_future()
//         .map_err(|(err, _stream)| err)
//         .and_then(move |(omsg, stream)| { match omsg {
//             None => future::ok(Break(())),
//             Some(msg) => {
//                 match single_step(msg, &mut dstate) {
//                     Err(err) => future::err(err),
//                     Ok(x) => match x {
//                         Some(out) => future::ok(Break(())),
//                         None => future::ok(Continue((stream, dstate)))
//                     }
//                 }
//             }
//         }})
//         .bxed()
// }

/// One step of the main loop.
fn loop_step(lstate: LoopState) -> BxFuture<future::Loop<(), LoopState>, Error> {
    use futures::future::Loop::{Break,Continue};
    let (stream, mut dstate) = lstate;
    // Note: There are many `bxed` calls in here because match arms must return the same type.
    // And the long long types produced by future chaining are not equivalent.
    // So the boxing turns them into trait objects implementing the same specified Future.
    stream
        .into_future()
        .map_err(|(err, _stream)| err)
        .and_then(move |(omsg, stream)| { match omsg {
            None => future::ok(Break(())).bxed(),
            Some(msg) => {
                match single_step(msg, &mut dstate) {
                    Err(err) => future::err(err).bxed(),
                    Ok(x) => match x {
                        Some(out) => {
                            stream.send(out).map(|stream| {
                                Continue((stream, dstate))
                            }).bxed()
                        }
                        None => future::ok(Continue((stream, dstate))).bxed()
                    }
                }
            }
        }}).bxed()
}

/// One synchronous step.
/// Returns a message to send.
fn single_step(msg: Message, dstate: &mut DownloaderState) -> Result<Option<Message>> {
    println!("message {}: {}", dstate.blah_state.nreceived, msg.summarize());
    dstate.blah_state.nreceived += 1;
    match msg {
        Message::Choke =>         dstate.peer_state.peer_choking = true,
        Message::Unchoke =>       dstate.peer_state.peer_choking = false,
        Message::Interested =>    dstate.peer_state.peer_interested = true,
        Message::NotInterested => dstate.peer_state.peer_interested = false,
        Message::Bitfield { bits } => {
            if bits.len() < dstate.info.num_pieces() {
                println!("{}", Error::new_str(&format!("bitfield has less bits {} than pieces {}",
                                                bits.len(), dstate.info.num_pieces())));
            }
            let mut i_start = 0;
            let mut in_interval = false;
            for b in 0..dstate.info.num_pieces() {
                if bits[b] && !in_interval {
                    i_start = b;
                    in_interval = true;
                } else if !bits[b] && in_interval {
                    dstate.peer_state.has.add(i_start as u32, b as u32)?;
                    in_interval = false;
                }
            }
            if in_interval {
                dstate.peer_state.has.add(i_start as u32, dstate.info.num_pieces() as u32)?;
            }
        },
        Message::Have { piece } => {
            dstate.peer_state.has.add(piece, piece+1)?;
        },
        Message::Piece { piece, offset, block } => {
            dstate.datastore.write_block(piece as usize, offset, &block)?;
            dstate.manifest.manifest.add_block(piece as usize, offset, block.len() as u32)?;
            dstate.manifest.store()?;
            dstate.blah_state.waiting = false;
        },
        _ => {},
    }
    if dstate.blah_state.nreceived >= 1 && dstate.peer_state.am_choking {
        let out = Message::Unchoke{};
        println!("sending message: {:?}", out);
        dstate.peer_state.am_choking = false;
        return Ok(Some(out));
    }
    if dstate.blah_state.nreceived >= 1 && !dstate.peer_state.am_interested {
        let out = Message::Interested{};
        println!("sending message: {:?}", out);
        dstate.peer_state.am_interested = true;
        return Ok(Some(out));
    }
    if !dstate.blah_state.waiting && !dstate.peer_state.peer_choking && dstate.peer_state.am_interested {
        match dstate.manifest.manifest.next_desired_block() {
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
                dstate.blah_state.waiting = true;
                return Ok(Some(out));
            }
        }
    }
    println!("state: {:?}", dstate.peer_state);

    Ok(None)
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
