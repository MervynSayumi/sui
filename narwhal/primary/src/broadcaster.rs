use std::{sync::Arc, time::Duration};

use anemo::{rpc::Status, Network, Request, Response};
use config::{AuthorityIdentifier, Committee};
use crypto::NetworkPublicKey;
use futures::{stream::FuturesOrdered, StreamExt};
use mysten_metrics::spawn_logged_monitored_task;
use network::{
    anemo_ext::{NetworkExt, WaitingPeer},
    client::NetworkClient,
};
use parking_lot::Mutex;
use tokio::{sync::broadcast, task::JoinSet, time::sleep};
use tracing::{error, trace, warn};
use types::{PrimaryToPrimaryClient, SendHeaderRequest, SendHeaderResponse, SignedHeader};

/// Broadcaster ensures headers are broadcasted to other primaries with retries for network errors.
/// Also, Broadcaster will keep broadcasting the latest header to help the network stay alive.
pub struct Broadcaster {
    inner: Arc<Inner>,
}

impl Broadcaster {
    pub(crate) fn new(
        authority_id: AuthorityIdentifier,
        committee: Committee,
        client: NetworkClient,
    ) -> Self {
        const BROADCAST_BACKLOG_CAPACITY: usize = 100;

        let (tx_own_header_broadcast, _rx_own_header_broadcast) =
            broadcast::channel(BROADCAST_BACKLOG_CAPACITY);
        let inner = Arc::new(Inner {
            authority_id,
            committee,
            client,
            header_senders: Default::default(),
            tx_own_header_broadcast: tx_own_header_broadcast.clone(),
        });

        // Initialize sender tasks asynchronously, to not block construction of Broadcaster.
        let inner_clone = inner.clone();
        spawn_logged_monitored_task!(
            async move {
                let Ok(network) = inner_clone.client.get_primary_network().await else {
                    error!("Failed to get primary Network!");
                    return;
                };
                let mut senders = inner_clone.header_senders.lock();
                for (peer_authority, _, peer_name) in inner_clone
                    .committee
                    .others_primaries_by_id(inner_clone.authority_id)
                    .into_iter()
                {
                    senders.spawn(Self::push_headers(
                        network.clone(),
                        peer_authority,
                        peer_name,
                        tx_own_header_broadcast.subscribe(),
                    ));
                }
            },
            "Broadcaster"
        );
        Self { inner }
    }

    pub(crate) fn broadcast_header(&self, signed_header: SignedHeader) {
        if let Err(e) = self.inner.tx_own_header_broadcast.send(signed_header) {
            warn!(
                "Failed to broadcast header. Likely all senders have exited. ({:?})",
                e
            );
        }
    }

    /// Runs a loop that continously pushes new headers received from the rx_own_header_broadcast
    /// channel to the target peer.
    ///
    /// Exits only when the primary is shutting down.
    async fn push_headers(
        network: Network,
        peer_authority: AuthorityIdentifier,
        peer_name: NetworkPublicKey,
        mut rx_own_header_broadcast: broadcast::Receiver<SignedHeader>,
    ) {
        const PUSH_TIMEOUT: Duration = Duration::from_secs(10);
        let peer_id = anemo::PeerId(peer_name.0.to_bytes());
        let peer = network.waiting_peer(peer_id);
        let client = PrimaryToPrimaryClient::new(peer);
        // Older broadcasts return early, so the last broadcast must be the latest header.
        // This will contain at most headers created within the last PUSH_TIMEOUT.
        let mut requests = FuturesOrdered::new();
        // Back off and retry only happen when there is only one header to be broadcasted.
        // Otherwise no retry happens.
        const BACKOFF_INTERVAL: Duration = Duration::from_millis(100);
        const MAX_BACKOFF_MULTIPLIER: u32 = 100;
        let mut backoff_multiplier: u32 = 0;

        async fn send_header(
            mut client: PrimaryToPrimaryClient<WaitingPeer>,
            request: Request<SendHeaderRequest>,
            header: SignedHeader,
        ) -> (SignedHeader, Result<Response<SendHeaderResponse>, Status>) {
            let resp = client.send_header(request).await;
            (header, resp)
        }

        loop {
            tokio::select! {
                result = rx_own_header_broadcast.recv() => {
                    let header = match result {
                        Ok(header) => header,
                        Err(broadcast::error::RecvError::Closed) => {
                            trace!("Sender to {peer_authority} is shutting down!");
                            return;
                        }
                        Err(broadcast::error::RecvError::Lagged(e)) => {
                            warn!("Sender to {peer_authority} is lagging! {e}");
                            // Re-run the loop to receive again.
                            continue;
                        }
                    };
                    let request = Request::new(SendHeaderRequest { signed_header: header.clone() }).with_timeout(PUSH_TIMEOUT);
                    requests.push_back(send_header(client.clone(),request, header));
                }
                Some((header, resp)) = requests.next() => {
                    backoff_multiplier = match resp {
                        Ok(_) => {
                            0
                        },
                        Err(_) => {
                            if requests.is_empty() {
                                // Retry broadcasting the latest header, to help the network stay alive.
                                let request = Request::new(SendHeaderRequest { signed_header: header.clone() }).with_timeout(PUSH_TIMEOUT);
                                requests.push_back(send_header(client.clone(), request, header));
                                std::cmp::min(backoff_multiplier * 2 + 1, MAX_BACKOFF_MULTIPLIER)
                            } else {
                                // TODO: add backoff and retries for transient & retriable errors.
                                0
                            }
                        },
                    };
                    if backoff_multiplier > 0 {
                        sleep(BACKOFF_INTERVAL * backoff_multiplier).await;
                    }
                }
            };
        }
    }
}

struct Inner {
    // The id of this primary.
    authority_id: AuthorityIdentifier,
    // Committee of the current epoch.
    committee: Committee,
    // Client for fetching payloads.
    client: NetworkClient,
    // Sender for broadcasting own headers.
    tx_own_header_broadcast: broadcast::Sender<SignedHeader>,
    // Background tasks proposing headers to peers.
    header_senders: Mutex<JoinSet<()>>,
}
