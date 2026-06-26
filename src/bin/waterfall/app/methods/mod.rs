//! `WaterfallApp` method modules.

mod connection {
    mod actions;
}
mod display {
    mod levels;
}
mod engine;
mod input;
mod pipeline;
mod plot {
    mod actions;
    mod cache;
    mod central;
    mod overlay;
    mod waterfall;
}
mod radio {
    mod sync;
}
mod settings;
mod spots {
    mod logic;
}
mod tuning;
mod ui {
    mod connection {
        mod airspy;
        mod card;
        mod form;
        mod kiwi_browser;
        mod kiwi_iq;
        mod qmx;
        mod recent;
        mod rtlsdr;
    }
    mod console;
    mod display;
    mod history;
    mod layout;
    mod left {
        mod cards;
        mod rf_controls;
    }
    mod popups;
    mod right {
        mod af_tuning;
        mod audio;
        mod cw_demod;
        mod performance;
    }
    mod spots {
        mod display;
        mod scp;
        mod skimmer;
    }
    mod status;
}
