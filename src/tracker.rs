use std::error::Error;
use ring::rand::SystemRandom;
use metainfo::*;

struct TrackerClient {}

impl TrackerClient {
}

pub const PEERID_SIZE: usize = 20;
pub type PeerID = [u8; PEERID_SIZE];

pub fn new_peer_id(rand: &SystemRandom) -> Result<PeerID,Box<Error>> {
    let mut id = [0; PEERID_SIZE];
    rand.fill(&mut id)?;
    id[0] = '-' as u8;
    id[1] = 'B' as u8;
    id[2] = 'I' as u8;
    id[3] = '0' as u8;
    id[4] = '0' as u8;
    id[5] = '0' as u8;
    id[6] = '1' as u8;
    Ok(id)
}

enum TrackerEvent {
    Started,
    Stopped,
    Completed,
    Periodical,
}

impl TrackerEvent {
    pub fn str(&self) -> Option<String> {
        match self {
            Started => Some("started".to_string()),
            Stopped => Some("stopped".to_string()),
            Completed => Some("completed".to_string()),
            Periodical => None,
        }
    }
}

// Parameters used in the client->tracker GET request
struct TrackerRequest {
    info_hash: InfoHash, // Hash of the 'info' section of the torrent file
    peer_id: PeerID, // Randomly generated peer id
    port: i64, // Port the client is listening. Typically in [6881-6889]
    uploaded: i64, // Bytes uploaded (since the client sent the 'started' event to the tracker)
    downloaded: i64, // Bytes downloaded (since the client sent the 'started' event to the tracker)
    left: i64, // Bytes left until 100% downloaded
    compact: bool, // Support a compact response
    no_peer_id: bool, // Response can omit peer id field in peers dictionary
    event: TrackerEvent,
    ip: Option<String>, // Outwardly-reachable IP of the client
    numwant: Option<i64>, // Number of peers requested
    key: Option<String>, // Identifier for this client with the tracker
    trackerid: Option<String>, // If a previous announce contained a tracker id, it should be set here
}

// The tracker responds with "text/plain" document consisting of a bencoded dictionary
struct TrackerResponse {
    // TODO
}
