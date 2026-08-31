use codex_core::models_manager::manager::ModelsManager;
use codex_protocol::config_types::CollaborationModeMask;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::TOP_LEVEL_OPERATING_MODES;

fn filtered_presets(models_manager: &ModelsManager) -> Vec<CollaborationModeMask> {
    models_manager
        .list_collaboration_modes()
        .into_iter()
        .filter(|mask| match mask.mode {
            Some(mode) => mode.is_tui_visible(),
            None => false,
        })
        .collect()
}

pub(crate) fn presets_for_tui(models_manager: &ModelsManager) -> Vec<CollaborationModeMask> {
    let presets = filtered_presets(models_manager);
    TOP_LEVEL_OPERATING_MODES
        .iter()
        .filter_map(|mode| {
            presets
                .iter()
                .find(|mask| mask.mode == Some(*mode))
                .cloned()
        })
        .collect()
}

pub(crate) fn default_mask(models_manager: &ModelsManager) -> Option<CollaborationModeMask> {
    let presets = filtered_presets(models_manager);
    presets
        .iter()
        .find(|mask| mask.mode == Some(ModeKind::Default))
        .cloned()
        .or_else(|| presets.into_iter().next())
}

pub(crate) fn mask_for_kind(
    models_manager: &ModelsManager,
    kind: ModeKind,
) -> Option<CollaborationModeMask> {
    if !kind.is_tui_visible() {
        return None;
    }
    filtered_presets(models_manager)
        .into_iter()
        .find(|mask| mask.mode == Some(kind))
}

/// Cycle to the next collaboration mode preset in list order.
pub(crate) fn next_mask(
    models_manager: &ModelsManager,
    current: Option<&CollaborationModeMask>,
) -> Option<CollaborationModeMask> {
    let presets = presets_for_tui(models_manager);
    if presets.is_empty() {
        return None;
    }
    let current_kind = current.and_then(|mask| mask.mode);
    let next_index = presets
        .iter()
        .position(|mask| mask.mode == current_kind)
        .map_or(0, |idx| (idx + 1) % presets.len());
    presets.get(next_index).cloned()
}

pub(crate) const fn description(kind: ModeKind) -> &'static str {
    match kind {
        ModeKind::Default => "Handle the work directly with one AI.",
        ModeKind::Agentic => "Delegate manually to configured agents when useful.",
        ModeKind::MultiAgent => "Automatically run the configured strategic workflow.",
        ModeKind::Plan | ModeKind::PairProgramming | ModeKind::Execute => "",
    }
}

pub(crate) fn default_mode_mask(models_manager: &ModelsManager) -> Option<CollaborationModeMask> {
    mask_for_kind(models_manager, ModeKind::Default)
}

pub(crate) fn plan_mask(models_manager: &ModelsManager) -> Option<CollaborationModeMask> {
    mask_for_kind(models_manager, ModeKind::Plan)
}
