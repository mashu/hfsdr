use hfsdr::MAX_NOTCHES;

use crate::iq_panel::IqPanel;
use crate::pipeline_flow::PipelineFlow;

pub struct ChromeState {
    pub show_console: bool,
    pub show_shortcuts: bool,
    pub show_af_scope: bool,
    pub show_smeter: bool,
    pub show_history: bool,
    pub show_left: bool,
    pub show_right: bool,
    pub show_iq_drawer: bool,
    pub show_pipeline_drawer: bool,
    pub pipeline_flow: PipelineFlow,
    pub notch_bypass_stash: Option<[bool; MAX_NOTCHES]>,
    pub iq: IqPanel,
    pub themed: bool,
}
