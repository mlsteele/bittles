use futures;
use futures::future::{Future};
use futures::future;
use futures::{Stream, Sink};
use std::cmp;
use std::collections::vec_deque::VecDeque;
use std::default::Default;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_core::net::TcpStream;
use tokio_core::reactor;
use tokio_core::reactor::Handle;
use tokio_io::AsyncRead;
use tokio_io::codec::Framed;

use datastore::DataStore;
use errors::*;
use fillable::*;
use manifest::ManifestWithFile;
use metainfo::{MetaInfo, InfoHash};
use peer_protocol::{PeerID, Message, BitTorrentPeerCodec};
use peer_protocol;
use tracker::TrackerClient;
use util::{tcp_connect2, BxFuture, FutureEnhanced, VecDequeStream, mkdirp_for_file};

type PeerFramed = Framed<TcpStream, BitTorrentPeerCodec>;

struct DownloaderState {
    info: MetaInfo,
    datastore: DataStore,
    manifest: ManifestWithFile,
    peer_state: PeerState,
    blah_state: BlahState,
}

type AM<T> = Arc<Mutex<T>>;

pub fn start<P: AsRef<Path>>(info: MetaInfo, peer_id: PeerID, store_path: P, manifest_path: P) -> Result<()> {
    let mut core = reactor::Core::new()?;
    let handle = core.handle();

    mkdirp_for_file(&store_path)?;
    mkdirp_for_file(&manifest_path)?;
    let datastore = DataStore::create_or_open(&info, store_path)?;
    let manifest = ManifestWithFile::load_or_new(info.clone(), manifest_path)?;
    let mut tc = TrackerClient::new(info.clone(), peer_id.clone())?;

    let tracker_res = tc.easy_start()?;
    println!("tracker res: {:#?}", tracker_res);
    if let Some(reason) = tracker_res.failure_reason {
        bail!("tracker failed: {}", reason);
    }

    if tracker_res.peers.is_empty() {
        bail!("tracker returned no peers");
    }

    let num_pieces = info.num_pieces() as u64;
    let info_hash = info.info_hash.clone();

    let n_start_peers = cmp::min(10, tracker_res.peers.len());
    println!("using {}/{} available peers", n_start_peers, tracker_res.peers.len());

    let dstate = DownloaderState {
        info: info,
        datastore: datastore,
        manifest: manifest,
        peer_state: PeerState::new(num_pieces),
        blah_state: BlahState::default(),
    };

    let dstate_c = Arc::new(Mutex::new(dstate));

    let mut top_futures: Vec<BxFuture<(), Error>> = Vec::new();

    for peer in tracker_res.peers[..n_start_peers].iter() {
        let dstate_c = dstate_c.clone();
        let handle = handle.clone();
        let f = connect_peer(peer.address, info_hash.clone(), peer_id.clone(), &handle)
            .then(move |res| match res {
                Err(err) => {
                    println!("could not connect to peer: {}", err);
                    future::ok(()).bxed()
                }
                Ok((stream, remote_peer_id)) => {
                    drive_peer(dstate_c, &handle, stream, remote_peer_id)
                }
            })
            .bxed();
        top_futures.push(f);
    }

    let future_root = future::join_all(top_futures);

    core.run(future_root)?;
    Ok(())
}

/// Connect to a remote peer
fn connect_peer(addr: SocketAddr, info_hash: InfoHash, peer_id: PeerID, handle: &reactor::Handle) -> BxFuture<(PeerFramed, PeerID), Error> {
    let info_hash2 = info_hash.clone();

    println!("connecting to {} ...", addr);
    tcp_connect2(&addr, Duration::from_millis(3000), &handle)
        .chain_err(|| "peer connection failed")
        .and_then(move |stream| {
            println!("connected");
            peer_protocol::handshake_send_async(stream, info_hash.clone(), peer_id.clone())
        })
        .and_then(|stream| peer_protocol::handshake_read_1_async(stream))
        .and_then(move |(stream, remote_info_hash)| {
            println!("remote info hash: {:?}", remote_info_hash);
            if remote_info_hash != info_hash2 {
                bail!("peer info hash mismatch peer:{:?} me:{:?}",
                      remote_info_hash,
                      info_hash2);
            }
            Ok(stream)
        })
        .and_then(|stream| peer_protocol::handshake_read_2_async(stream).map(move |(stream, remote_peer_id)| (stream, remote_peer_id)))
        .and_then(|(stream, remote_peer_id)| {
            println!("remote peer id: {:?}", remote_peer_id);

            // let stream: Framed<TcpStream,BitTorrentPeerCodec> = stream.framed(BitTorrentPeerCodec);
            let stream: PeerFramed = stream.framed(BitTorrentPeerCodec);
            Ok((stream, remote_peer_id))
        })
        .bxed()
}

enum HandlePeerMessageRes {
    Pass,
    Reply(VecDeque<Message>),
    Close,
}

fn drive_peer(dstate_c: AM<DownloaderState>, handle: &Handle, stream: PeerFramed, remote_peer_id: PeerID) -> BxFuture<(), Error> {
    let (peer_tx, peer_rx) = stream.split();

    // Separate send from receive so that the listener doesn't block all the time
    let (buf_tx, buf_rx) = futures::sync::mpsc::channel::<Message>(5);

    // debugging type assertions
    // let _: &Stream<Item = Message, Error = ()> = &buf_rx;
    // let _: &Sink<SinkItem = Message, SinkError = Error> = &peer_tx;
    // let _: &Stream<Item = Message, Error = Error> = &buf_rx.map_err(|()| "lol".into());
    // let _: &Future<Item = (_, _), Error = Error> = &peer_tx.send_all(buf_rx.map_err(|()| "lol".into()));

    // Process the send channel
    handle.spawn(peer_tx.send_all(buf_rx.map_err(|()| Into::<Error>::into("peer send failed")))
        .map(|(_sink, _stream)| ())
        .map_err(|e| {
            println!("warning: send to peer failed: {}", e);
        }));

    // type PeerStream = Box<Stream<Item = Message, Error = Error>>;
    // type PeerSink = Box<Sink<SinkItem = Message, SinkError = Error>>;
    // type LoopState = (AM<DownloaderState>, PeerStream, PeerSink);

    struct LoopState<S, U>
        where S: Stream<Item = Message, Error = Error>,
              U: Sink<SinkItem = Message, SinkError = Error>
    {
        dstate_c: AM<DownloaderState>,
        peer_rx: S,
        peer_tx: U,
    }

    let init = LoopState {
        dstate_c: dstate_c,
        peer_rx: peer_rx,
        peer_tx: buf_tx.sink_map_err(|send_err| {
            format!("error sending message: {}", send_err).into()
        }),
    };

    use futures::future::Loop;
    future::loop_fn(init,
                    |LoopState { dstate_c, peer_rx, peer_tx }| -> BxFuture<Loop<(), LoopState<_, _>>, Error> {
        peer_rx.into_future()
            .map_err(|(err, _stream)| err)
            .and_then(|(item, peer_rx)| -> BxFuture<Loop<(), LoopState<_, _>>, Error> {
                match item {
                    Some(msg) => {
                        let cmd: Result<HandlePeerMessageRes> = {
                            let mut dstate = dstate_c.lock().unwrap();
                            handle_peer_message(&mut dstate, &msg)
                        };
                        use self::HandlePeerMessageRes::*;
                        match cmd {
                            Ok(Pass) => future::ok(Loop::Continue(LoopState { dstate_c, peer_rx, peer_tx })).bxed(),
                            Ok(Reply(outs)) => {
                                peer_tx.send_all(VecDequeStream::<Message, Error>::new(outs))
                                    .map(move |(peer_tx, _)| Loop::Continue(LoopState { dstate_c, peer_rx, peer_tx }))
                                    .bxed()
                            }
                            Ok(Close) => {
                                // println!("closing peer connection: {:?}", remote_peer_id.clone());
                                println!("closing peer connection");
                                future::ok(Loop::Break(())).bxed()
                            }
                            Err(err) => {
                                println!("closing peer due to error: {:?}", err);
                                future::ok(Loop::Break(())).bxed()
                            }
                        }
                    }
                    None => {
                        println!("peer hung up");
                        future::ok(Loop::Break(())).bxed()
                    }
                }
            }).bxed()
    })
        .bxed()
}

/// One synchronous step.
/// Returns messages to send.
fn handle_peer_message(dstate: &mut DownloaderState, msg: &Message) -> Result<HandlePeerMessageRes> {
    use self::HandlePeerMessageRes::*;

    let mut outs = VecDeque::new();
    println!("recv message {}: {}",
             dstate.blah_state.nreceived,
             msg.summarize());
    dstate.blah_state.nreceived += 1;
    match msg {
        &Message::KeepAlive => {}
        &Message::Choke => dstate.peer_state.peer_choking = true,
        &Message::Unchoke => dstate.peer_state.peer_choking = false,
        &Message::Interested => dstate.peer_state.peer_interested = true,
        &Message::NotInterested => dstate.peer_state.peer_interested = false,
        &Message::Bitfield { ref bits } => {
            if bits.len() < dstate.info.num_pieces() {
                bail!("bitfield has less bits {} than pieces {}",
                      bits.len(),
                      dstate.info.num_pieces());
            }
            let mut i_start = 0;
            let mut in_interval = false;
            for b in 0..dstate.info.num_pieces() {
                if bits[b] && !in_interval {
                    i_start = b;
                    in_interval = true;
                } else if !bits[b] && in_interval {
                    dstate.peer_state.has.add(i_start as u64, b as u64)?;
                    in_interval = false;
                }
            }
            if in_interval {
                dstate.peer_state.has.add(i_start as u64, dstate.info.num_pieces() as u64)?;
            }
        }
        &Message::Have { piece } => {
            dstate.peer_state.has.add(piece as u64, piece as u64 + 1)?;
        }
        &Message::Request { .. } => {
            bail!("not implemented");
        }
        &Message::Piece { piece, offset, ref block } => {
            dstate.datastore.write_block(piece as u64, offset as u64, &block)?;
            let newly_filled = dstate.manifest.manifest.add_block(piece as u64, offset as u64, block.len() as u64)?;
            for p in newly_filled {
                let expected_hash = dstate.info.piece_hashes[p as usize].clone();
                println!("filled piece: {}", p);
                if let Some(verified) = dstate.datastore.verify_piece(p, expected_hash)? {
                    println!("verified piece: {}", verified.piece);
                    dstate.manifest.manifest.mark_verified(verified)?;
                } else {
                    println!("flunked piece: {}", p);
                    dstate.manifest.manifest.remove_piece(p)?;
                }
            }

            dstate.manifest.store()?;
            dstate.blah_state.waiting = false;
        }
        &Message::Cancel { .. } => bail!("not implemented"),
        &Message::Port { .. } => {}
    }
    if dstate.blah_state.nreceived >= 1 && dstate.peer_state.am_choking {
        let out = Message::Unchoke {};
        println!("sending message: {:?}", out);
        dstate.peer_state.am_choking = false;
        outs.push_back(out);
    }
    if dstate.blah_state.nreceived >= 1 && !dstate.peer_state.am_interested {
        let out = Message::Interested {};
        println!("sending message: {:?}", out);
        dstate.peer_state.am_interested = true;
        outs.push_back(out);
    }
    if !dstate.blah_state.waiting && !dstate.peer_state.peer_choking && dstate.peer_state.am_interested {
        match dstate.manifest.manifest.next_desired_block() {
            None => {
                // No more blocks needed! Unless something fails verification.
                for piece in dstate.manifest.manifest.needs_verify() {
                    let expected_hash = dstate.info.piece_hashes[piece as usize].clone();
                    println!("verifying piece: {}", piece);
                    if let Some(verified) = dstate.datastore.verify_piece(piece, expected_hash)? {
                        println!("verified piece: {}", verified.piece);
                        dstate.manifest.manifest.mark_verified(verified)?;
                    } else {
                        println!("flunked piece: {}", piece);
                        dstate.manifest.manifest.remove_piece(piece)?;
                    }
                }
                dstate.manifest.store()?;

                if dstate.manifest.manifest.all_verified() {
                    println!("all pieces verified!");
                    return Ok(Close);
                }
            }
            Some(desire) => {
                let out = Message::Request {
                    piece: desire.piece,
                    offset: desire.offset,
                    length: desire.length,
                };
                println!("sending message: {:?}", out);
                dstate.blah_state.waiting = true;
                outs.push_back(out);
            }
        }
    }

    if outs.len() > 0 {
        Ok(Reply(outs))
    } else {
        Ok(Pass)
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

    has: Fillable,
}

#[derive(Debug, Default)]
pub struct BlahState {
    nreceived: u64,
    requested: bool,
    waiting: bool,
}

impl PeerState {
    fn new(num_pieces: u64) -> Self {
        PeerState {
            peer_interested: false,
            peer_choking: true,
            am_interested: false,
            am_choking: true,
            has: Fillable::new(num_pieces),
        }
    }
}
