use datastore::DataStore;
use errors::*;
use fillable::*;
use futures;
use futures::{Sink, Stream};
use futures::future;
use futures::future::Future;
use manifest::{BlockRequest, ManifestWithFile};
use metainfo::{InfoHash, MetaInfo};
use peer_protocol;
use peer_protocol::{BitTorrentPeerCodec, Message, PeerID};
use slog::Logger;
use std::cmp;
use std::collections::{HashMap, HashSet, VecDeque};
use std::default::Default;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
use tokio_core::net::TcpStream;
use tokio_core::reactor;
use tokio_core::reactor::Handle;
use tokio_io::AsyncRead;
use tokio_io::codec::Framed;
use tracker;
use tracker::TrackerClient;
use util::{BxFuture, FutureEnhanced, VecDequeStream, mkdirp_for_file, tcp_connect2};

type PeerFramed = Framed<TcpStream, BitTorrentPeerCodec>;

// Local number used to identify peer connections.
type PeerNum = usize;

struct DownloaderState {
    info: MetaInfo,
    datastore: DataStore,
    manifest: ManifestWithFile,
    peer_states: HashMap<PeerNum, PeerState>,
    next_peer_num: AtomicUsize,
    outstanding: OutstandingRequestsManager,
}

type AM<T> = Arc<Mutex<T>>;

pub fn start<P: AsRef<Path>>(log: Logger, info: MetaInfo, peer_id: PeerID, store_path: P, manifest_path: P) -> Result<()> {
    let log2 = log.clone();

    let mut core = reactor::Core::new()?;
    let handle = core.handle();

    mkdirp_for_file(&store_path)?;
    mkdirp_for_file(&manifest_path)?;
    let datastore = DataStore::create_or_open(&info, store_path)?;
    let manifest = ManifestWithFile::load_or_new(log2, info.clone(), manifest_path)?;
    let mut tc = TrackerClient::new(info.clone(), peer_id.clone())?;

    debug!(log, "asking tracker");
    let tracker_res = tc.easy_start()?;
    debug!(log, "tracker res: {:#?}", tracker_res);
    if let Some(reason) = tracker_res.failure_reason {
        bail!("tracker failed: {}", reason);
    }

    if tracker_res.peers.is_empty() {
        bail!("tracker returned no peers");
    }

    let num_pieces = info.num_pieces() as u64;
    let info_hash = info.info_hash.clone();

    let dstate = DownloaderState {
        info: info,
        datastore: datastore,
        manifest: manifest,
        peer_states: HashMap::new(),
        next_peer_num: AtomicUsize::new(0),
        outstanding: OutstandingRequestsManager::new(),
    };

    let dstate_c = Arc::new(Mutex::new(dstate));

    let mut top_futures: Vec<BxFuture<(), Error>> = Vec::new();

    top_futures.push(run_progress_report(log.clone(), handle.clone(), dstate_c.clone()));

    let n_start_peers = cmp::min(15, tracker_res.peers.len());
    info!(log,
           "using {}/{} available peers",
           n_start_peers,
           tracker_res.peers.len());

    // Connect to an initial set of peers
    for peer in tracker_res.peers[..n_start_peers].iter() {
        let peer_num = {
            let dstate = dstate_c.lock().unwrap();
            dstate
                .next_peer_num
                .fetch_add(1, ::std::sync::atomic::Ordering::Relaxed)
        };
        let dstate_c = dstate_c.clone();
        let handle = handle.clone();
        let log = log.new(o!("peer_num" => peer_num));
        let f = run_peer(log.clone(),
                         handle.clone(),
                         dstate_c,
                         peer.clone(),
                         info_hash.clone(),
                         num_pieces,
                         peer_id.clone(),
                         peer_num);
        top_futures.push(f);
    }

    let future_root = future::join_all(top_futures);

    core.run(future_root)?;
    Ok(())
}


/// Run a loop that prints a progress report occasionally.
fn run_progress_report(log: Logger, handle: reactor::Handle, dstate_c: AM<DownloaderState>) -> BxFuture<(), Error> {
    use futures::future::Loop::{Break, Continue};

    future::loop_fn((log, handle, dstate_c), |(log, handle, dstate_c)| {
        let duration = Duration::from_millis(500);
        match reactor::Timeout::new(duration, &handle) {
            Err(err) => future::err(Into::<Error>::into(err)).bxed(),
            Ok(timeout) => timeout
                .map_err(|e| e.into())
                .and_then(|()| {
                    let pres = {
                        let mut dstate = dstate_c.lock().unwrap();
                        progress_report(log.clone(), &mut dstate)
                    };
                    match pres {
                        Err(err) => Err(Into::<Error>::into(err)),
                        Ok(true) => Ok(Continue((log, handle, dstate_c))),
                        Ok(false) => Ok(Break(())),
                    }
                }).bxed(),
        }
    })
            .bxed()
}

/// Returns whether to continue looping.
fn progress_report(log: Logger, dstate: &mut DownloaderState) -> Result<bool> {
    let chokers = dstate
        .peer_states
        .iter()
        .filter(|&(ref _k, ref v)| v.peer_choking)
        .count();
    let n_peers = dstate.peer_states.len();

    // let bar = dstate.manifest.manifest.progress_bar();
    // info!(log, "progress report: {}", bar);
    let p = dstate.manifest.manifest.amount_verified();
    info!(log,
          "progress: {:03}%  chokers:{}/{}",
          p * (100 as f64),
          chokers,
          n_peers);

    let go = !dstate.manifest.manifest.is_all_verified();
    Ok(go)
}

/// Connect and run a peer.
/// Connects and runs the peer loop.
/// Logs most errors. Any returned error is a programming error.
fn run_peer(log: Logger,
            handle: reactor::Handle,
            dstate_c: AM<DownloaderState>,
            peer: tracker::Peer,
            info_hash: InfoHash,
            num_pieces: u64,
            local_peer_id: PeerID,
            peer_num: PeerNum)
            -> BxFuture<(), Error> {

    let log2 = log.clone();
    connect_peer(&log,
                 peer.address,
                 info_hash.clone(),
                 local_peer_id.clone(),
                 peer_num,
                 &handle)
            .and_then(move |(stream, remote_peer_id)| {
                // Create peer state
                {
                    let mut dstate = dstate_c.lock().unwrap();
                    let x = dstate
                        .peer_states
                        .insert(peer_num, PeerState::new(num_pieces, remote_peer_id));
                    if x.is_some() {
                        warn!(log, "peer state already existed"; "peer_num" => peer_num)
                    }
                }

                let dstate_c2 = dstate_c.clone();
                drive_peer(&log, dstate_c, &handle, stream, peer_num)
                    .then(move |res| {
                        // Delete peer state
                        let mut dstate = dstate_c2.lock().unwrap();
                        let x = dstate.peer_states.remove(&peer_num);
                        if x.is_none() {
                            warn!(log,  "peer state was missing"; "peer_num" => peer_num)
                        }

                        let n = dstate.outstanding.clear_peer(peer_num);
                        debug!(log, "cleared outstanding requests: {}", n; "peer_num" => peer_num);

                        res
                    })
                    .bxed()
            })
            .or_else(move |err| {
                         error!(log2, "peer error: {}", err);
                         Ok(())
                     })
            .bxed()
}

/// Connect to a remote peer
fn connect_peer(log: &Logger, addr: SocketAddr, info_hash: InfoHash, peer_id: PeerID, peer_num: PeerNum, handle: &reactor::Handle) -> BxFuture<(PeerFramed, PeerID), Error> {
    let info_hash2 = info_hash.clone();

    let log1 = log.clone();
    let log2 = log.clone();
    let log3 = log.clone();

    info!(log, "connecting to {} ...", addr);
    tcp_connect2(&addr, Duration::from_millis(3000), &handle)
        .chain_err(|| "peer connection failed")
        .and_then(move |stream| {
                      info!(log1, "connected");
                      peer_protocol::handshake_send_async(stream, info_hash.clone(), peer_id.clone())
                  })
        .and_then(|stream| peer_protocol::handshake_read_1_async(stream))
        .and_then(move |(stream, remote_info_hash)| {
            debug!(log2, "remote info hash: {:?}", remote_info_hash);
            if remote_info_hash != info_hash2 {
                bail!("peer [{}] info hash mismatch peer:{:?} me:{:?}",
                      peer_num,
                      remote_info_hash,
                      info_hash2);
            }
            Ok(stream)
        })
        .and_then(|stream| peer_protocol::handshake_read_2_async(stream).map(move |(stream, remote_peer_id)| (stream, remote_peer_id)))
        .and_then(move |(stream, remote_peer_id)| {
                      debug!(log3, "remote peer id: {:?}", remote_peer_id);

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

fn drive_peer(log: &Logger, dstate_c: AM<DownloaderState>, handle: &Handle, stream: PeerFramed, peer_num: PeerNum) -> BxFuture<(), Error> {
    let (peer_tx, peer_rx) = stream.split();

    // Separate send from receive so that the listener doesn't block all the time
    let (buf_tx, buf_rx) = futures::sync::mpsc::channel::<Message>(5);

    // debugging type assertions
    // let _: &Stream<Item = Message, Error = ()> = &buf_rx;
    // let _: &Sink<SinkItem = Message, SinkError = Error> = &peer_tx;
    // let _: &Stream<Item = Message, Error = Error> = &buf_rx.map_err(|()| "lol".into());
    // let _: &Future<Item = (_, _), Error = Error> = &peer_tx.send_all(buf_rx.map_err(|()| "lol".into()));

    // Process the send channel
    let log2 = log.clone();
    handle.spawn(peer_tx.send_all(buf_rx.map_err(|()| Into::<Error>::into("peer send failed")))
        .map(|(_sink, _stream)| ())
        .map_err(move |e| {
            warn!(log2, "warning: send to peer failed: {}", e);
        }));

    // type PeerStream = Box<Stream<Item = Message, Error = Error>>;
    // type PeerSink = Box<Sink<SinkItem = Message, SinkError = Error>>;
    // type LoopState = (AM<DownloaderState>, PeerStream, PeerSink);

    struct LoopState<S, U>
        where S: Stream<Item = Message, Error = Error>,
              U: Sink<SinkItem = Message, SinkError = Error>
    {
        log: Logger,
        dstate_c: AM<DownloaderState>,
        peer_num: PeerNum,
        peer_rx: S,
        peer_tx: U,
    }

    let init = LoopState {
        log: log.clone(),
        dstate_c: dstate_c,
        peer_num: peer_num,
        peer_rx: peer_rx,
        peer_tx: buf_tx.sink_map_err(|send_err| format!("error sending message: {}", send_err).into()),
    };

    use futures::future::Loop;
    future::loop_fn(init, |LoopState {
                         log,
                         peer_num,
                         dstate_c,
                         peer_rx,
                         peer_tx,
                     }|
     -> BxFuture<Loop<(), LoopState<_, _>>, Error> {
        peer_rx
            .into_future()
            .map_err(|(err, _stream)| err)
            .and_then(move |(item, peer_rx)| -> BxFuture<Loop<(), LoopState<_, _>>, Error> {
                match item {
                    Some(msg) => {
                        let cmd: Result<HandlePeerMessageRes> = {
                            let mut dstate = dstate_c.lock().unwrap();
                            handle_peer_message(&log, &mut dstate, peer_num, &msg)
                        };
                        use self::HandlePeerMessageRes::*;
                        match cmd {
                            Ok(Pass) => {
                                future::ok(Loop::Continue(LoopState {
                                                              log,
                                                              dstate_c,
                                                              peer_num,
                                                              peer_rx,
                                                              peer_tx,
                                                          }))
                                        .bxed()
                            }
                            Ok(Reply(outs)) => {
                                peer_tx
                                    .send_all(VecDequeStream::<Message, Error>::new(outs))
                                    .map(move |(peer_tx, _)| {
                                        Loop::Continue(LoopState {
                                                           log,
                                                           dstate_c,
                                                           peer_num,
                                                           peer_rx,
                                                           peer_tx,
                                                       })
                                    })
                                    .bxed()
                            }
                            Ok(Close) => {
                                debug!(log, "closing peer connection");
                                future::ok(Loop::Break(())).bxed()
                            }
                            Err(err) => {
                                error!(log, "closing peer due to error: {:?}", err);
                                future::ok(Loop::Break(())).bxed()
                            }
                        }
                    }
                    None => {
                        debug!(log, "peer hung up");
                        future::ok(Loop::Break(())).bxed()
                    }
                }
            })
            .bxed()
    })
            .bxed()
}

/// One synchronous step.
/// Returns messages to send.
fn handle_peer_message(log: &Logger, dstate: &mut DownloaderState, peer_num: PeerNum, msg: &Message) -> Result<HandlePeerMessageRes> {
    use self::HandlePeerMessageRes::*;

    debug!(log, "n-out {}", dstate.outstanding.get_num(peer_num));

    let rstate = dstate
        .peer_states
        .get_mut(&peer_num)
        .ok_or_else(|| Into::<Error>::into(format!("missing peer state: {}", peer_num).to_owned()))?;

    let mut outs = VecDeque::new();
    debug!(log, "recv message";
           "msg" => msg.summarize(),
           "n" => rstate.temp.nreceived);
    rstate.temp.nreceived += 1;
    match msg {
        &Message::KeepAlive => {}
        &Message::Choke => {
            rstate.peer_choking = true;
            dstate.outstanding.clear_peer(peer_num);
        }
        &Message::Unchoke => {
            rstate.peer_choking = false;
            dstate.outstanding.clear_peer(peer_num);
        }
        &Message::Interested => rstate.peer_interested = true,
        &Message::NotInterested => rstate.peer_interested = false,
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
                    rstate.has.add(i_start as u64, b as u64)?;
                    in_interval = false;
                }
            }
            if in_interval {
                rstate
                    .has
                    .add(i_start as u64, dstate.info.num_pieces() as u64)?;
            }
        }
        &Message::Have { piece } => {
            rstate.has.add(piece as u64, piece as u64 + 1)?;
        }
        &Message::Request { .. } => {
            bail!("not implemented");
        }
        &Message::Piece {
            piece,
            offset,
            ref block,
        } => {
            dstate
                .datastore
                .write_block(piece as u64, offset as u64, &block)?;
            let newly_filled = dstate
                .manifest
                .manifest
                .add_block(piece as u64, offset as u64, block.len() as u64)?;
            for p in newly_filled {
                let expected_hash = dstate.info.piece_hashes[p as usize].clone();
                info!(log, "filled piece: {}", p);
                if let Some(verified) = dstate.datastore.verify_piece(p, expected_hash)? {
                    info!(log, "verified piece: {}", verified.piece);
                    dstate.manifest.manifest.mark_verified(verified)?;
                } else {
                    info!(log, "flunked piece: {}", p);
                    dstate.manifest.manifest.remove_piece(p)?;
                }
            }

            dstate
                .outstanding
                .clear(peer_num,
                       BlockRequest {
                           piece: piece as u64,
                           offset: offset as u64,
                           length: block.len() as u64,
                       });

            dstate.manifest.store(log)?;
        }
        &Message::Cancel { .. } => bail!("not implemented"),
        &Message::Port { .. } => {}
    }

    if rstate.temp.nreceived >= 1 && rstate.am_choking {
        let out = Message::Unchoke {};
        debug!(log, "sending message: {:?}", out);
        rstate.am_choking = false;
        outs.push_back(out);
    }
    if rstate.temp.nreceived >= 1 && !rstate.am_interested {
        let out = Message::Interested {};
        debug!(log, "sending message: {:?}", out);
        rstate.am_interested = true;
        outs.push_back(out);
    }
    if !rstate.peer_choking && rstate.am_interested {
        for safety in 0.. {
            if safety == 99 {
                error!(log, "collecting too many requests to send!");
            }
            match next_request(log, &mut dstate.manifest, &mut dstate.outstanding, peer_num)? {
                None => {
                    if dstate.manifest.manifest.is_all_full() {
                        verify_all(log,
                                   &dstate.info,
                                   &mut dstate.manifest,
                                   &mut dstate.datastore)?;

                        if dstate.manifest.manifest.is_all_verified() {
                            println!("all pieces verified!");
                            return Ok(Close);
                        }
                    } else {
                        if safety == 0 {
                            debug!(log, "not requesting");
                        }
                    }
                    break;
                }
                Some(desire) => {
                    let out = Message::Request {
                        piece: desire.piece as u32,
                        offset: desire.offset as u32,
                        length: desire.length as u32,
                    };
                    dstate.outstanding.add(peer_num, desire);
                    debug!(log, "sending message: {:?}", out);
                    outs.push_back(out);
                }
            }
        }
    }

    if outs.len() > 0 {
        Ok(Reply(outs))
    } else {
        Ok(Pass)
    }
}

/// Decide the next block to request from a peer.
fn next_request(log: &Logger, manifest: &mut ManifestWithFile, outstanding: &mut OutstandingRequestsManager, peer_num: PeerNum) -> Result<Option<BlockRequest>> {
    const MAX_OUTSTANDING_PER_PEER: u64 = 5;
    const MAX_OUTSTANDING_PER_BLOCK: u64 = 1;
    if outstanding.get_num(peer_num) >= MAX_OUTSTANDING_PER_PEER {
        // Already plenty of requests outstanding on this peer.
        return Ok(None);
    }
    let mut after = None;
    for safety in 0.. {
        if safety == 99 {
            error!(log, "loop has gone too far looking for next request!");
        }

        if let Some(desire) = manifest.manifest.next_desired_block(log, after) {
            // TODO: allow multiple outstanding per block, maybe, and if so remember to cancel upon receive.
            let ps = outstanding.get_peers(desire);
            if ps.len() > MAX_OUTSTANDING_PER_BLOCK as usize {
                after = Some(desire);
                continue;
            }
            if ps.contains(&peer_num) {
                after = Some(desire);
                continue;
            }
            return Ok(Some(desire));
        }
        return Ok(None);
    }
    Ok(None)
}

/// Call this when the download might be done.
/// Run verification on the data, save the manifest.
/// If this function returns Ok that does _not_ mean all verified.
fn verify_all(log: &Logger, info: &MetaInfo, manifest: &mut ManifestWithFile, datastore: &mut DataStore) -> Result<()> {
    // No more blocks needed! Unless something fails verification.
    for piece in manifest.manifest.needs_verify() {
        let expected_hash = info.piece_hashes[piece as usize].clone();
        info!(log, "verifying piece: {}", piece);
        if let Some(verified) = datastore.verify_piece(piece, expected_hash)? {
            info!(log, "verified piece: {}", verified.piece);
            manifest.manifest.mark_verified(verified)?;
        } else {
            info!(log, "flunked piece: {}", piece);
            manifest.manifest.remove_piece(piece)?;
        }
    }
    manifest.store(log)?;
    Ok(())
}

#[derive(Debug)]
pub struct PeerState {
    /// ID of the remote peer
    peer_id: PeerID,

    /// Peer is interested in this client
    peer_interested: bool,
    /// Peer is choking this client
    peer_choking: bool,

    am_interested: bool,
    am_choking: bool,

    has: Fillable,

    temp: TempState,
}

#[derive(Debug, Default)]
pub struct TempState {
    nreceived: u64,
}

impl PeerState {
    fn new(num_pieces: u64, peer_id: PeerID) -> Self {
        PeerState {
            peer_id: peer_id,
            peer_interested: false,
            peer_choking: true,
            am_interested: false,
            am_choking: true,
            has: Fillable::new(num_pieces),
            temp: TempState::default(),
        }
    }
}

struct OutstandingRequestsManager {
    /// Blocks for each peer.
    peer_blocks: HashMap<PeerNum, HashSet<BlockRequest>>,
    /// Peers for each block
    block_peers: HashMap<BlockRequest, HashSet<PeerNum>>,
}

impl OutstandingRequestsManager {
    fn new() -> Self {
        Self {
            peer_blocks: HashMap::new(),
            block_peers: HashMap::new(),
        }
    }

    fn add(&mut self, peer: PeerNum, block: BlockRequest) {
        self.peer_blocks
            .entry(peer)
            .or_insert(HashSet::new())
            .insert(block);

        self.block_peers
            .entry(block)
            .or_insert(HashSet::new())
            .insert(peer);
    }

    /// Returns the number of cleared items: 0 or 1.
    fn clear(&mut self, peer: PeerNum, block: BlockRequest) -> usize {
        if let Some(peers) = self.block_peers.get_mut(&block) {
            peers.remove(&peer);

            if let Some(blocks) = self.peer_blocks.get_mut(&peer) {
                blocks.remove(&block);
            }

            return 1;
        } else {
            return 0;
        }
    }

    /// Clear all outstanding requests for a peer.
    /// Returns the number of cleared items.
    fn clear_peer(&mut self, peer: PeerNum) -> usize {
        if let Some(blocks) = self.peer_blocks.remove(&peer) {
            let mut x = 0;
            for block in blocks.iter() {
                if let Some(peers) = self.block_peers.get_mut(&block) {
                    peers.remove(&peer);
                    x += 1;
                }
            }
            return x;
        } else {
            return 0;
        }
    }

    /// Get the set of peers with outstanding requests for the block
    fn get_peers(&self, block: BlockRequest) -> HashSet<PeerNum> {
        return self.block_peers
                   .get(&block)
                   .map(|x| x.clone())
                   .unwrap_or_else(|| HashSet::new());
    }

    /// Get the number of requests outstanding for the peer
    fn get_num(&self, peer: PeerNum) -> u64 {
        return self.peer_blocks
                   .get(&peer)
                   .map(|x| x.len() as u64)
                   .unwrap_or(0);
    }
}
