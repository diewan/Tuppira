//! WebSocket subscription service for real-time wallet updates.
//!
//! Allows wallets to subscribe to address-specific events
//! and receive real-time notifications when new sanads, seals,
//! or transfers are indexed.

use std::collections::HashMap;
use std::sync::Arc;

use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};
use warp::ws::{WebSocket, Message, Ws};
use warp::Filter;

/// Types of events that can be broadcast to subscribers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SubscriptionEvent {
    /// New sanad indexed for an address.
    NewSanad {
        address: String,
        chain: String,
        sanad_id: String,
        data: serde_json::Value,
    },
    /// New seal indexed for an address.
    NewSeal {
        address: String,
        chain: String,
        seal_id: String,
        data: serde_json::Value,
    },
    /// New transfer indexed for an address.
    NewTransfer {
        address: String,
        chain: String,
        transfer_id: String,
        data: serde_json::Value,
    },
    /// Indexing completed for an address.
    IndexingComplete {
        address: String,
        chain: String,
        sanads_count: u64,
        seals_count: u64,
        transfers_count: u64,
    },
    /// Indexing error for an address.
    IndexingError {
        address: String,
        chain: String,
        error: String,
    },
}

/// Subscription request from a WebSocket client.
#[derive(Debug, Deserialize)]
pub struct SubscriptionRequest {
    /// Action to perform (subscribe/unsubscribe).
    pub action: String,
    /// Address to subscribe to.
    pub address: String,
    /// Chain the address belongs to.
    pub chain: String,
    /// Network (mainnet/testnet).
    pub network: Option<String>,
}

/// Response sent back to the client.
#[derive(Debug, Serialize)]
pub struct SubscriptionResponse {
    /// Whether the subscription action was successful.
    pub success: bool,
    /// Message describing the result.
    pub message: String,
    /// Event data (if this is an event notification).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<SubscriptionEvent>,
}

/// Manages WebSocket connections and event broadcasting.
#[derive(Clone)]
pub struct SubscriptionManager {
    /// Active subscriptions per address.
    subscriptions: Arc<RwLock<HashMap<String, Vec<mpsc::UnboundedSender<SubscriptionEvent>>>>>,
}

impl SubscriptionManager {
    /// Create a new subscription manager.
    pub fn new() -> Self {
        Self {
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Subscribe to events for a specific address.
    pub async fn subscribe(&self, address: &str) -> mpsc::UnboundedReceiver<SubscriptionEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        
        let mut subs = self.subscriptions.write().await;
        subs.entry(address.to_string())
            .or_insert_with(Vec::new)
            .push(tx);

        tracing::info!(address = %address, "New subscription added");
        rx
    }

    /// Unsubscribe from events for a specific address.
    pub async fn unsubscribe(&self, address: &str, tx: &mpsc::UnboundedSender<SubscriptionEvent>) {
        let mut subs = self.subscriptions.write().await;
        
        if let Some(subscribers) = subs.get_mut(address) {
            subscribers.retain(|s| !s.same_channel(tx));
            
            if subscribers.is_empty() {
                subs.remove(address);
                tracing::info!(address = %address, "Subscription removed");
            }
        }
    }

    /// Broadcast an event to all subscribers of an address.
    pub async fn broadcast(&self, address: &str, event: SubscriptionEvent) {
        let subs = self.subscriptions.read().await;
        
        if let Some(subscribers) = subs.get(address) {
            let mut failed_indexes = Vec::new();
            
            for (i, tx) in subscribers.iter().enumerate() {
                if tx.send(event.clone()).is_err() {
                    failed_indexes.push(i);
                }
            }

            // Remove failed subscribers
            if !failed_indexes.is_empty() {
                drop(subs);
                let mut subs = self.subscriptions.write().await;
                if let Some(subscribers) = subs.get_mut(address) {
                    for i in failed_indexes.into_iter().rev() {
                        subscribers.remove(i);
                    }
                    if subscribers.is_empty() {
                        subs.remove(address);
                    }
                }
            }

            tracing::debug!(address = %address, event_type = %get_event_type(&event), "Event broadcast");
        }
    }

    /// Broadcast a new sanad event.
    pub async fn broadcast_new_sanad(
        &self,
        address: &str,
        chain: &str,
        sanad_id: &str,
        data: serde_json::Value,
    ) {
        let event = SubscriptionEvent::NewSanad {
            address: address.to_string(),
            chain: chain.to_string(),
            sanad_id: sanad_id.to_string(),
            data,
        };
        self.broadcast(address, event).await;
    }

    /// Broadcast a new seal event.
    pub async fn broadcast_new_seal(
        &self,
        address: &str,
        chain: &str,
        seal_id: &str,
        data: serde_json::Value,
    ) {
        let event = SubscriptionEvent::NewSeal {
            address: address.to_string(),
            chain: chain.to_string(),
            seal_id: seal_id.to_string(),
            data,
        };
        self.broadcast(address, event).await;
    }

    /// Broadcast a new transfer event.
    pub async fn broadcast_new_transfer(
        &self,
        address: &str,
        chain: &str,
        transfer_id: &str,
        data: serde_json::Value,
    ) {
        let event = SubscriptionEvent::NewTransfer {
            address: address.to_string(),
            chain: chain.to_string(),
            transfer_id: transfer_id.to_string(),
            data,
        };
        self.broadcast(address, event).await;
    }

    /// Get the number of active subscriptions.
    pub async fn subscription_count(&self) -> usize {
        let subs = self.subscriptions.read().await;
        subs.values().map(|v| v.len()).sum()
    }
}

/// Get the event type as a string.
fn get_event_type(event: &SubscriptionEvent) -> &'static str {
    match event {
        SubscriptionEvent::NewSanad { .. } => "new_sanad",
        SubscriptionEvent::NewSeal { .. } => "new_seal",
        SubscriptionEvent::NewTransfer { .. } => "new_transfer",
        SubscriptionEvent::IndexingComplete { .. } => "indexing_complete",
        SubscriptionEvent::IndexingError { .. } => "indexing_error",
    }
}

/// Handle a WebSocket connection for subscriptions.
pub async fn handle_subscription(
    ws: Ws,
    manager: SubscriptionManager,
) -> Result<impl warp::Reply, warp::Rejection> {
    Ok(ws.on_upgrade(move |websocket| handle_websocket(websocket, manager)))
}

/// Handle the WebSocket connection.
async fn handle_websocket(websocket: WebSocket, manager: SubscriptionManager) {
    let (mut ws_sender, mut ws_receiver) = websocket.split();
    
    // Channel for receiving events from the subscription manager
    let mut active_subscriptions: HashMap<String, mpsc::UnboundedReceiver<SubscriptionEvent>> = HashMap::new();
    
    // Task to forward events from subscriptions to the WebSocket
    let mut subscription_tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    loop {
        tokio::select! {
            // Handle incoming messages from the client
            Some(Ok(message)) = ws_receiver.next() => {
                if let Ok(text) = message.to_str() {
                    if let Ok(request) = serde_json::from_str::<SubscriptionRequest>(text) {
                        match request.action.as_str() {
                            "subscribe" => {
                                let rx = manager.subscribe(&request.address).await;
                                active_subscriptions.insert(request.address.clone(), rx);
                                
                                let response = SubscriptionResponse {
                                    success: true,
                                    message: format!("Subscribed to {}", request.address),
                                    event: None,
                                };
                                
                                if let Ok(json) = serde_json::to_string(&response) {
                                    let _ = ws_sender.send(Message::text(json)).await;
                                }
                            }
                            "unsubscribe" => {
                                active_subscriptions.remove(&request.address);
                                
                                let response = SubscriptionResponse {
                                    success: true,
                                    message: format!("Unsubscribed from {}", request.address),
                                    event: None,
                                };
                                
                                if let Ok(json) = serde_json::to_string(&response) {
                                    let _ = ws_sender.send(Message::text(json)).await;
                                }
                            }
                            _ => {
                                let response = SubscriptionResponse {
                                    success: false,
                                    message: "Invalid action. Use 'subscribe' or 'unsubscribe'".to_string(),
                                    event: None,
                                };
                                
                                if let Ok(json) = serde_json::to_string(&response) {
                                    let _ = ws_sender.send(Message::text(json)).await;
                                }
                            }
                        }
                    }
                }
            }
            // Handle subscription events
            _ = async {
                for (address, rx) in active_subscriptions.iter_mut() {
                    if let Ok(event) = rx.try_recv() {
                        let response = SubscriptionResponse {
                            success: true,
                            message: "Event received".to_string(),
                            event: Some(event),
                        };
                        
                        if let Ok(json) = serde_json::to_string(&response) {
                            let _ = ws_sender.send(Message::text(json)).await;
                        }
                    }
                }
                std::future::pending::<()>().await
            } => {}
            // Connection closed
            else => break,
        }
    }

    // Clean up subscription tasks
    for task in subscription_tasks {
        task.abort();
    }

    tracing::info!("WebSocket connection closed");
}

/// Create the WebSocket endpoint route.
pub fn subscription_ws(
    manager: SubscriptionManager,
) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    warp::path("ws")
        .and(warp::path("subscriptions"))
        .and(warp::ws())
        .and(with_subscription_manager(manager))
        .and_then(handle_subscription)
}

/// Warp filter to pass the subscription manager to handlers.
fn with_subscription_manager(
    manager: SubscriptionManager,
) -> impl Filter<Extract = (SubscriptionManager,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || manager.clone())
}
