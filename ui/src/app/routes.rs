/// Routes module for the CSV Explorer UI.
use dioxus::prelude::*;
use dioxus_router::*;

use crate::pages::{
    Chains, ContractsList, Home, SanadDetail, SanadsList, SealDetail, SealsList, Stats,
    TransferDetail, TransfersList, Wallet,
};

#[derive(Routable, Clone, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[route("/")]
    Home {},
    #[route("/sanads")]
    SanadsList {},
    #[route("/sanads/:id")]
    SanadDetail { id: String },
    #[route("/transfers")]
    TransfersList {},
    #[route("/transfers/:id")]
    TransferDetail { id: String },
    #[route("/seals")]
    SealsList {},
    #[route("/seals/:id")]
    SealDetail { id: String },
    #[route("/contracts")]
    ContractsList {},
    #[route("/stats")]
    Stats {},
    #[route("/chains")]
    Chains {},
    #[route("/wallet")]
    Wallet {},
}
