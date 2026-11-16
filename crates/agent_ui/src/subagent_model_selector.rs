use std::sync::Arc;

use agent_settings::AgentSettings;
use collections::IndexMap;
use fs::Fs;
use gpui::{AnyElement, App, DismissEvent, Subscription, Task};
use language_model::{IconOrSvg, LanguageModelProviderId, LanguageModelRegistry};
use picker::{Picker, PickerDelegate};
use settings::{
    AllowedSubagentModels, Settings, SettingsStore, SubagentModel, update_settings_file,
};
use ui::prelude::*;

use crate::language_model_selector::{ModelInfo, ModelMatcher};
use crate::ui::{ModelSelectorHeader, ModelSelectorListItem};

pub type SubagentModelSelector = Picker<SubagentModelSelectorDelegate>;

/// The picker behind the subagent button in the message editor. It edits
/// `agent.allowed_subagent_models` and `agent.selected_subagent_models`.
pub fn subagent_model_selector(
    window: &mut Window,
    cx: &mut Context<SubagentModelSelector>,
) -> SubagentModelSelector {
    let mut delegate = SubagentModelSelectorDelegate {
        fs: <dyn Fs>::global(cx),
        entries: Vec::new(),
        selected_index: 0,
        query: String::new(),
        // The rows and their checked state come from settings, which are written
        // asynchronously.
        _subscriptions: vec![
            cx.observe_global_in::<SettingsStore>(window, |picker, window, cx| {
                picker.refresh(window, cx)
            }),
            cx.subscribe_in(
                &LanguageModelRegistry::global(cx),
                window,
                |picker, _, event, window, cx| {
                    if matches!(
                        event,
                        language_model::Event::ProviderStateChanged(_)
                            | language_model::Event::AddedProvider(_)
                            | language_model::Event::RemovedProvider(_)
                            | language_model::Event::ProvidersChanged
                    ) {
                        picker.refresh(window, cx);
                    }
                },
            ),
        ],
    };
    delegate.update_entries("", cx);
    Picker::list(delegate, window, cx)
        .show_scrollbar(true)
        .initial_width(rems(20.))
        .popover()
}

pub struct SubagentModelSelectorDelegate {
    fs: Arc<dyn Fs>,
    entries: Vec<SubagentModelEntry>,
    selected_index: usize,
    query: String,
    _subscriptions: Vec<Subscription>,
}

enum SubagentModelEntry {
    Header {
        title: SharedString,
        has_border: bool,
    },
    Rule(AllowedSubagentModels),
    Model(ModelInfo),
}

const RULES: [(AllowedSubagentModels, &str); 4] = [
    (AllowedSubagentModels::Any, "Any Model"),
    (AllowedSubagentModels::SameProvider, "Same Provider"),
    (AllowedSubagentModels::SelectedOnly, "Selected Only"),
    (AllowedSubagentModels::NoModels, "None"),
];

impl SubagentModelSelectorDelegate {
    fn update_entries(&mut self, query: &str, cx: &App) {
        // The selected models only apply under "Selected Only", so they are
        // only listed there.
        let models = if AgentSettings::get_global(cx).allowed_subagent_models
            == AllowedSubagentModels::SelectedOnly
        {
            LanguageModelRegistry::read_global(cx)
                .visible_providers()
                .into_iter()
                .filter(|provider| provider.is_authenticated(cx))
                .flat_map(|provider| {
                    provider
                        .provided_models(cx)
                        .into_iter()
                        .map(move |model| ModelInfo::new(&*provider, model, &Default::default()))
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        let mut entries = Vec::new();
        let models = if query.is_empty() || models.is_empty() {
            entries.push(SubagentModelEntry::Header {
                title: "Allowed Subagent Models".into(),
                has_border: false,
            });
            entries.extend(
                RULES
                    .iter()
                    .map(|(rule, _)| SubagentModelEntry::Rule(*rule)),
            );
            if !models.is_empty() {
                entries.push(SubagentModelEntry::Header {
                    title: "Selected Models".into(),
                    has_border: true,
                });
            }
            models
        } else {
            ModelMatcher::new(
                models,
                cx.foreground_executor().clone(),
                cx.background_executor().clone(),
            )
            .fuzzy_search(query)
        };

        let mut models_by_provider: IndexMap<LanguageModelProviderId, Vec<ModelInfo>> =
            IndexMap::default();
        for model in models {
            models_by_provider
                .entry(model.model.provider_id())
                .or_default()
                .push(model);
        }
        for models in models_by_provider.into_values() {
            let Some(first_model) = models.first() else {
                continue;
            };
            entries.push(SubagentModelEntry::Header {
                title: first_model.model.provider_name().0,
                has_border: matches!(
                    entries.last(),
                    Some(SubagentModelEntry::Model(_) | SubagentModelEntry::Rule(_))
                ),
            });
            entries.extend(models.into_iter().map(SubagentModelEntry::Model));
        }

        // Provider updates can reorder rows without changing the query.
        let selected_entry = self.entries.get(self.selected_index);
        let selected_index = if query == self.query {
            entries
                .iter()
                .position(|entry| match (selected_entry, entry) {
                    (Some(SubagentModelEntry::Rule(selected)), SubagentModelEntry::Rule(rule)) => {
                        selected == rule
                    }
                    (
                        Some(SubagentModelEntry::Model(selected)),
                        SubagentModelEntry::Model(model),
                    ) => selected.model.is_same_as(&model.model),
                    _ => false,
                })
        } else {
            None
        };
        self.selected_index = selected_index
            .or_else(|| {
                entries
                    .iter()
                    .position(|entry| !matches!(entry, SubagentModelEntry::Header { .. }))
            })
            .unwrap_or(0);
        self.query = query.to_string();
        self.entries = entries;
    }
}

impl PickerDelegate for SubagentModelSelectorDelegate {
    type ListItem = AnyElement;

    fn name() -> &'static str {
        "subagent model selector"
    }

    fn match_count(&self) -> usize {
        self.entries.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(&mut self, ix: usize, _: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.selected_index = ix.min(self.entries.len().saturating_sub(1));
        cx.notify();
    }

    fn can_select(&self, ix: usize, _window: &mut Window, _cx: &mut Context<Picker<Self>>) -> bool {
        matches!(
            self.entries.get(ix),
            Some(SubagentModelEntry::Rule(_) | SubagentModelEntry::Model(_))
        )
    }

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Search models…".into()
    }

    // There is nothing to search until the models are listed.
    fn render_editor(
        &self,
        editor: &Arc<dyn picker::ErasedEditor>,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<Div> {
        (AgentSettings::get_global(cx).allowed_subagent_models
            != AllowedSubagentModels::SelectedOnly)
            .then(|| div().track_focus(&editor.focus_handle(cx)))
    }

    fn update_matches(
        &mut self,
        query: String,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        self.update_entries(&query, cx);
        cx.notify();
        Task::ready(())
    }

    // Choices apply immediately and the menu stays open, so several models can be toggled.
    fn confirm(&mut self, _secondary: bool, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        match self.entries.get(self.selected_index) {
            Some(SubagentModelEntry::Rule(rule)) => {
                let rule = *rule;
                update_settings_file(self.fs.clone(), cx, move |settings, _cx| {
                    settings
                        .agent
                        .get_or_insert_default()
                        .allowed_subagent_models = Some(rule);
                });
            }
            Some(SubagentModelEntry::Model(model_info)) => {
                let model = SubagentModel {
                    provider: model_info.model.provider_id().0.to_string().into(),
                    model: model_info.model.id().0.to_string(),
                };
                update_settings_file(self.fs.clone(), cx, move |settings, _cx| {
                    let agent = settings.agent.get_or_insert_default();
                    let selected_models = agent.selected_subagent_models.get_or_insert_default();
                    match selected_models
                        .iter()
                        .position(|selected| *selected == model)
                    {
                        Some(index) => {
                            selected_models.remove(index);
                        }
                        None => selected_models.push(model),
                    }
                });
            }
            Some(SubagentModelEntry::Header { .. }) | None => {}
        }
    }

    fn dismissed(&mut self, _: &mut Window, cx: &mut Context<Picker<Self>>) {
        cx.emit(DismissEvent);
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let agent_settings = AgentSettings::get_global(cx);
        Some(match self.entries.get(ix)? {
            SubagentModelEntry::Header { title, has_border } => {
                ModelSelectorHeader::new(title.clone(), *has_border).into_any_element()
            }
            SubagentModelEntry::Rule(rule) => {
                let (_, label) = RULES.iter().find(|(candidate, _)| candidate == rule)?;
                ModelSelectorListItem::new(ix, *label)
                    .is_selected(agent_settings.allowed_subagent_models == *rule)
                    .is_focused(selected)
                    .into_any_element()
            }
            SubagentModelEntry::Model(model_info) => {
                ModelSelectorListItem::new(ix, model_info.model.name().0)
                    .map(|this| match &model_info.icon {
                        IconOrSvg::Icon(icon_name) => this.icon(*icon_name),
                        IconOrSvg::Svg(icon_path) => this.icon_path(icon_path.clone()),
                    })
                    .is_selected(
                        agent_settings
                            .selected_subagent_models
                            .iter()
                            .any(|selected| {
                                selected.matches_model(
                                    &model_info.model.provider_id().0,
                                    &model_info.model.id().0,
                                )
                            }),
                    )
                    .is_focused(selected)
                    .is_latest(model_info.model.is_latest())
                    .into_any_element()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs::FakeFs;
    use gpui::{Focusable, KeyBinding, TestAppContext};

    #[gpui::test]
    async fn test_subagent_model_selector_keyboard_and_settings(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        fs.create_dir(paths::config_dir()).await.unwrap();
        fs.insert_file(paths::settings_file(), b"{}".to_vec()).await;
        cx.update(|cx| {
            let store = SettingsStore::test(cx);
            cx.set_global(store);
            <dyn Fs>::set_global(fs.clone(), cx);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            editor::init(cx);
            let provider = LanguageModelRegistry::test(cx);
            provider.model("chosen-model");
            cx.bind_keys([
                KeyBinding::new("down", menu::SelectNext, Some("Picker")),
                KeyBinding::new("up", menu::SelectPrevious, Some("Picker")),
                KeyBinding::new("enter", menu::Confirm, Some("Picker")),
                KeyBinding::new("escape", menu::Cancel, Some("Picker")),
            ]);
        });
        let (picker, cx) = cx.add_window_view(subagent_model_selector);
        picker.update_in(cx, |picker, window, cx| {
            window.focus(&picker.focus_handle(cx), cx)
        });
        cx.run_until_parked();

        cx.simulate_keystrokes("down enter");
        cx.read(|cx| {
            assert_eq!(
                AgentSettings::get_global(cx).allowed_subagent_models,
                AllowedSubagentModels::SameProvider
            )
        });
        cx.simulate_keystrokes("down enter");
        cx.read(|cx| {
            assert_eq!(
                AgentSettings::get_global(cx).allowed_subagent_models,
                AllowedSubagentModels::SelectedOnly
            )
        });

        cx.simulate_input("chosen");
        let provider = Arc::new(
            language_model::fake_provider::FakeLanguageModelProvider::new(
                LanguageModelProviderId::from("team/api".to_string()),
                language_model::LanguageModelProviderName::from("Team API".to_string()),
            ),
        );
        provider.model("chosen-other");
        cx.update(|_, cx| {
            LanguageModelRegistry::global(cx).update(cx, |registry, cx| {
                registry.register_provider(provider, cx);
            });
        });
        cx.run_until_parked();
        picker.read_with(cx, |picker, cx| {
            assert_eq!(picker.query(cx), "chosen");
            assert_eq!(picker.delegate.match_count(), 4);
        });
        picker.update_in(cx, |picker, window, cx| picker.set_query("", window, cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("down down down down down");
        let preceding_provider = Arc::new(
            language_model::fake_provider::FakeLanguageModelProvider::new(
                LanguageModelProviderId::from("a-provider".to_string()),
                language_model::LanguageModelProviderName::from("A Provider".to_string()),
            ),
        );
        preceding_provider.model("chosen-before");
        cx.update(|_, cx| {
            LanguageModelRegistry::global(cx).update(cx, |registry, cx| {
                registry.register_provider(preceding_provider, cx);
            })
        });
        cx.run_until_parked();
        picker.read_with(cx, |picker, _| {
            assert!(
                matches!(picker.delegate.entries.get(picker.delegate.selected_index()),
                Some(SubagentModelEntry::Model(model))
                    if &*model.model.provider_id().0 == "fake" && &*model.model.id().0 == "chosen-model")
            );
        });
        cx.update(|_, cx| {
            LanguageModelRegistry::global(cx).update(cx, |registry, cx| {
                registry.unregister_provider("a-provider".to_string().into(), cx);
            })
        });
        cx.run_until_parked();
        picker.read_with(cx, |picker, _| {
            assert!(
                matches!(picker.delegate.entries.get(picker.delegate.selected_index()),
                Some(SubagentModelEntry::Model(model))
                    if &*model.model.provider_id().0 == "fake" && &*model.model.id().0 == "chosen-model")
            );
        });
        picker.update_in(cx, |picker, window, cx| {
            picker.set_query("chosen", window, cx)
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("down enter");
        cx.read(|cx| {
            assert_eq!(
                AgentSettings::get_global(cx).selected_subagent_models,
                [SubagentModel {
                    provider: "team/api".to_string().into(),
                    model: "chosen-other".to_string(),
                }]
            );
        });
        cx.simulate_keystrokes("enter up");
        cx.update(|_, cx| {
            LanguageModelRegistry::global(cx).update(cx, |registry, cx| {
                registry.unregister_provider("team/api".to_string().into(), cx);
            });
        });
        cx.run_until_parked();
        picker.read_with(cx, |picker, _| {
            assert_eq!(picker.delegate.match_count(), 2);
            assert!(matches!(picker.delegate.entries.get(1), Some(SubagentModelEntry::Model(model)) if &*model.model.id().0 == "chosen-model"));
        });
        cx.simulate_keystrokes("enter");
        cx.read(|cx| {
            assert_eq!(
                AgentSettings::get_global(cx).selected_subagent_models,
                [SubagentModel {
                    provider: "fake".to_string().into(),
                    model: "chosen-model".to_string(),
                }]
            )
        });
        let saved: serde_json::Value =
            serde_json::from_str(&fs.load(paths::settings_file()).await.unwrap()).unwrap();
        assert_eq!(
            saved["agent"]["selected_subagent_models"],
            serde_json::json!([{"provider": "fake", "model": "chosen-model"}])
        );
        cx.simulate_keystrokes("enter");
        cx.read(|cx| {
            assert!(
                AgentSettings::get_global(cx)
                    .selected_subagent_models
                    .is_empty()
            )
        });

        picker.update_in(cx, |picker, window, cx| picker.set_query("", window, cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("down down down enter");
        cx.read(|cx| {
            assert_eq!(
                AgentSettings::get_global(cx).allowed_subagent_models,
                AllowedSubagentModels::NoModels
            )
        });
        cx.simulate_keystrokes("up up up enter");
        cx.read(|cx| {
            assert_eq!(
                AgentSettings::get_global(cx).allowed_subagent_models,
                AllowedSubagentModels::Any
            )
        });

        let dismissed = std::rc::Rc::new(std::cell::Cell::new(false));
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&picker, {
                let dismissed = dismissed.clone();
                move |_, _: &DismissEvent, _| dismissed.set(true)
            })
        });
        cx.simulate_keystrokes("escape");
        assert!(dismissed.get());
    }
}
