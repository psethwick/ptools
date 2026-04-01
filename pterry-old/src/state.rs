use std::{
    collections::BTreeMap,
    ops::Deref,
    sync::{
        mpsc::{Receiver, Sender},
        Arc,
    },
};

use crate::{
    core::Core,
    language::{parse, Verb},
    picker::{
        self, next_picker_by_name, Action, Item::PickerStackOp, ListItem, Picker, PickerOp, ToCore,
        ToMeatspace, VerbHandler,
    },
};
use anyhow::{anyhow, Result};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use itertools::Itertools;
pub struct State {
    pub root_picker: Arc<dyn Picker>,
    pub current_picker: Arc<dyn Picker>,
    pub cached: Vec<ListItem>,
    pub verbs: BTreeMap<Verb, VerbHandler>,
    pub picker_stack: Vec<String>,
}

fn collect_list_items(p: &dyn Picker) -> Vec<ListItem> {
    p.cacheable()
        .unwrap_or_default()
        .iter()
        .map(<ListItem as Clone>::clone)
        .collect()
}

fn collect_verbs(p: &dyn Picker) -> BTreeMap<Verb, VerbHandler> {
    p.verbs().unwrap_or_default().into_iter().collect()
}

impl State {
    pub fn event_loop(
        &mut self,
        loopback: Sender<ToCore>,
        irx: Receiver<ToCore>,
        ctx: Sender<ToMeatspace>,
    ) {
        let matcher = SkimMatcherV2::default();
        dbg!("state start loop");
        loop {
            if let Ok(input) = irx.recv() {
                dbg!("state loop");
                match input {
                    ToCore::Exit => {
                        let _ = ctx.send(ToMeatspace::Close);
                    }
                    ToCore::InputChanged(s) => {
                        let mut to_send: Vec<ListItem> = self
                            .cached
                            .clone()
                            .into_iter()
                            .filter_map(|it| Some((matcher.fuzzy_indices(&it.display, &s)?, it)))
                            .sorted_by_key(|x| -x.0 .0)
                            .map(|(fi, mut it)| {
                                it.highlights = fi.1.into();
                                it
                            })
                            .collect();
                        // TODO: should it be dynamic first??
                        // maybe order should be configurable
                        if let Some(d) = self.current_picker.dynamic(&s) {
                            to_send.extend(d);
                        }

                        if let Some(sp) = self.current_picker.sub_pickers() {
                            to_send.extend(sp.into_iter().map(|p| ListItem {
                                display: format!("{} picker", p.name()),
                                highlights: None,
                                item: picker::Item::PickerStackOp(picker::PickerOp::Push(
                                    p.name().to_owned(),
                                )),
                            }));
                        }
                        let _ = ctx.send(ToMeatspace::Items(to_send));
                    }
                    ToCore::Selected(item) => {
                        match item {
                            PickerStackOp(po) => {
                                if self.change_picker(po).is_err() {
                                    let _ = ctx.send(ToMeatspace::Close);
                                }
                                let _ = ctx.send(ToMeatspace::Clear);
                                if let Some(list_items) = self.current_picker.cacheable() {
                                    let _ = ctx.send(ToMeatspace::Items(list_items));
                                } else {
                                    let _ = ctx.send(ToMeatspace::Items(vec![]));
                                }
                            }
                            item => {
                                dbg!("tocore Selected");
                                match self.current_picker.action(&item) {
                                    Ok(Some(m)) => match m {
                                        Action::Selected(it) => {
                                            let _ = loopback.send(ToCore::Selected(it));
                                            // unfortunately weird but whatever for now
                                        }
                                        Action::Close => {
                                            let _ = ctx.send(ToMeatspace::Close);
                                        }
                                    },
                                    Ok(None) => {}
                                    Err(e) => {
                                        dbg!(e);
                                    }
                                };
                            }
                        }
                    }
                    ToCore::HandleVerb(i) => {
                        let parsed = parse(&i);
                        if let Some(verb) = parsed.verb {
                            if let Some(verb_handler) = self.verbs.get(&verb) {
                                if let Ok(Some(o)) = verb_handler(&parsed.text) {
                                    let _ = ctx.send(o);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn change_picker(&mut self, op: PickerOp) -> Result<()> {
        let new_picker = match op {
            PickerOp::Pop => {
                if self.picker_stack.pop().is_none() {
                    return Err(anyhow!("stack empty"));
                }
                let mut new_picker = self.current_picker.clone();
                for picker_name in &self.picker_stack {
                    if let Some(np) = next_picker_by_name(new_picker.clone(), picker_name) {
                        new_picker = np;
                    }
                }
                new_picker
            }

            #[allow(clippy::unwrap_or_default)]
            PickerOp::Push(name) => {
                self.picker_stack
                    .push(self.current_picker.name().to_owned());
                next_picker_by_name(self.current_picker.clone(), &name)
                    .unwrap_or(Arc::<Core>::default())
            }
        };
        self.set_picker(new_picker);
        Ok(())
    }

    pub fn set_picker(&mut self, new_picker: Arc<dyn Picker>) {
        self.root_picker = new_picker.clone();
        self.current_picker = new_picker;
        self.cached = collect_list_items(self.current_picker.deref());
        self.verbs = collect_verbs(self.current_picker.deref());
    }
}

impl Default for State {
    fn default() -> Self {
        let root_picker = Arc::<Core>::default();
        let current_picker = Arc::<Core>::default();
        let cached: Vec<_> = collect_list_items(current_picker.deref());
        let verbs = collect_verbs(current_picker.deref());
        let picker_stack = vec![];

        Self {
            root_picker,
            current_picker,
            cached,
            verbs,
            picker_stack,
        }
    }
}
