use std::sync::mpsc;
use std::thread;
use std::io;
use std::net;
use error::{Result};
use metainfo::{InfoHash};
use peer_protocol::{PeerID};
use peer_protocol;

pub enum Command {
    Close,
    Message(peer_protocol::Message),
}

pub enum Event {
    Handshake(InfoHash, PeerID),
    Message(peer_protocol::Message),
    Closed(ClosedCause),
}

/// The reason the peer closed.
/// Ideally, this would be an Error.
/// But I couldn't figure out how to make that Send, so we're stuck with this.
pub enum ClosedCause {
    Command,
    CommandChannelClosed,
    HandshakeError,
    ReadError,
    SendError,
    StreamCloneError,
}

pub struct PeerHandleArgs {
    info_hash: InfoHash,
    peer_id: PeerID,
}

pub struct PeerHandle {
    /// Channel to send commands
    tx: mpsc::SyncSender<Command>,
    /// Channel to receive events
    rx: mpsc::Receiver<Event>,
}

impl PeerHandle {
    pub fn new(stream: net::TcpStream, args: PeerHandleArgs) -> PeerHandle
        // where S: io::Write + io::Read
    {
        let (cmd_tx, cmd_rx) = mpsc::sync_channel(100);
        let (mut eve_tx, eve_rx) = mpsc::sync_channel(100);
        thread::spawn(move|| {
            let stop_cause: ClosedCause = Self::run(args, stream, cmd_rx, &mut eve_tx);
            let _ = eve_tx.send(Event::Closed(stop_cause));
        });
        // TODO set read and write timeout on the stream
        PeerHandle {
            tx: cmd_tx,
            rx: eve_rx,
        }
    }

    fn handshake<S>(args: PeerHandleArgs,
                    mut stream: S,
                    eve_tx: &mpsc::SyncSender<Event>)
                    -> Result<()>
        where S: io::Write + io::Read
    {

        peer_protocol::handshake_send(&mut stream, args.info_hash, args.peer_id)?;
        stream.flush()?;

        let remote_info_hash = peer_protocol::handshake_read_1(&mut stream)?;
        let remote_peer_id = peer_protocol::handshake_read_2(&mut stream)?;
        eve_tx.send(Event::Handshake(remote_info_hash, remote_peer_id))?;

        Ok(())
    }

    /// Run the handler thread.
    /// When an error occurs, just exits. It's up to the caller to send Event::Closed.
    fn run(args: PeerHandleArgs,
           mut stream: net::TcpStream,
           cmd_rx: mpsc::Receiver<Command>,
           eve_tx: &mut mpsc::SyncSender<Event>)
           -> ClosedCause
    {
        match Self::handshake(args, &mut stream, &eve_tx) {
            Ok(_) => {},
            Err(err) => {
                let _ = err;
                return ClosedCause::HandshakeError;
            }
        }

        // A channel used to merge the two types of closing.
        // 1. cmd_rx: close command
        // 2. stream: read failure
        let (coop_close_tx1, coop_close_rx) = mpsc::sync_channel::<ClosedCause>(0);
        let coop_close_tx2 = coop_close_tx1.clone();

        let mut stream2 = match stream.try_clone() {
            Ok(x) => x,
            Err(err) => {
                let _ = err;
                return ClosedCause::StreamCloneError;
            }
        };

        thread::spawn(move|| {
            loop {
                match cmd_rx.recv() {
                    Err(err) => {
                        let _ = err;
                        let _ = coop_close_tx1.send(ClosedCause::CommandChannelClosed);
                        break;
                    },
                    Ok(cmd) => match cmd {
                        Command::Close => {
                            let _ = coop_close_tx1.send(ClosedCause::Command);
                            break;
                        },
                        Command::Message(out) => {
                            let res = peer_protocol::send_message(&mut stream2, &out);
                            if let Err(err) = res {
                                let _ = err;
                                let _ = coop_close_tx1.send(ClosedCause::SendError);
                                break;
                            }
                        },
                    },
                }
            }
        });

        thread::spawn(move|| {
            loop {
                match peer_protocol::read_message(&mut stream) {
                    Ok(m) => {
                        let _ = eve_tx.send(Event::Message(m));
                    },
                    Err(err) => {
                        let _ = err;
                        let _ = coop_close_tx2.send(ClosedCause::ReadError);
                        break;
                    }
                }
            }
        });

        let stop_cause = coop_close_rx.recv().unwrap();
        stop_cause
    }
}
