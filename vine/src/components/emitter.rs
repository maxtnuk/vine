use std::{collections::BTreeMap, fmt::Write, mem::take};

use ivy::ast::{Net, Nets, Tree};
use vine_util::idx::{Counter, IdxVec};

use crate::{
  components::analyzer::usage::Usage,
  structures::{
    chart::{Chart, EnumDef, VariantId},
    resolutions::{Fragment, FragmentId},
    specializations::{Spec, SpecId, Specializations},
    tir::Local,
    vir::{
      Header, Interface, InterfaceKind, Invocation, Port, Stage, StageId, Step, Transfer, Vir,
    },
  },
};

pub struct Emitter<'core, 'a> {
  pub nets: Nets,
  chart: &'a Chart<'core>,
  specs: &'a Specializations,
  fragments: &'a IdxVec<FragmentId, Fragment<'core>>,
  vir: &'a IdxVec<FragmentId, Vir>,
  dup_labels: Counter<usize>,
}

impl<'core, 'a> Emitter<'core, 'a> {
  pub fn new(
    chart: &'a Chart<'core>,
    specs: &'a Specializations,
    fragments: &'a IdxVec<FragmentId, Fragment<'core>>,
    vir: &'a IdxVec<FragmentId, Vir>,
  ) -> Self {
    Emitter { nets: Nets::default(), chart, specs, fragments, vir, dup_labels: Counter::default() }
  }

  pub fn emit_main(&mut self, main: FragmentId) {
    let path = self.fragments[main].path;
    let vir = &self.vir[main];
    let func = *vir.closures.last().unwrap();
    let InterfaceKind::Fn { call, .. } = vir.interfaces[func].kind else { unreachable!() };
    let global = format!("{path}::{}", call.0);
    self.nets.insert("::".into(), Net { root: Tree::Global(global), pairs: Vec::new() });
  }

  pub fn emit_spec(&mut self, spec_id: SpecId) {
    let spec = self.specs.specs[spec_id].as_ref().unwrap();
    let vir = &self.vir[spec.fragment];

    let mut emitter = VirEmitter {
      chart: self.chart,
      specs: self.specs,
      fragments: self.fragments,
      spec_id,
      spec,
      vir,
      locals: BTreeMap::new(),
      pairs: Vec::new(),
      wire_offset: 0,
      wires: Counter::default(),
      dup_labels: self.dup_labels,
    };

    for stage in vir.stages.values() {
      let interface = &vir.interfaces[stage.interface];
      if interface.incoming != 0 && !interface.inline() {
        emitter.wire_offset = 0;
        emitter.wires.0 = stage.wires.0 .0;
        let root = emitter.emit_interface(interface, true);
        let root = emitter.emit_header(&stage.header, root);
        emitter._emit_stage(stage);
        for (_, local) in take(&mut emitter.locals) {
          emitter.finish_local(local);
        }
        let net = Net { root, pairs: take(&mut emitter.pairs) };
        self.nets.insert(emitter.stage_name(spec_id, stage.id), net);
      }
    }

    self.dup_labels = emitter.dup_labels;
  }
}

struct VirEmitter<'core, 'a> {
  chart: &'a Chart<'core>,
  specs: &'a Specializations,
  fragments: &'a IdxVec<FragmentId, Fragment<'core>>,
  spec_id: SpecId,
  spec: &'a Spec,
  vir: &'a Vir,
  locals: BTreeMap<Local, LocalState>,
  pairs: Vec<(Tree, Tree)>,
  wire_offset: usize,
  wires: Counter<usize>,
  dup_labels: Counter<usize>,
}

impl<'core, 'a> VirEmitter<'core, 'a> {
  fn dup_label(&mut self) -> String {
    format!("dup{}", self.dup_labels.next())
  }

  pub fn emit_transfer(&mut self, transfer: &Transfer) {
    let interface = &self.vir.interfaces[transfer.interface];
    if interface.inline() {
      let InterfaceKind::Unconditional(stage) = interface.kind else { unreachable!() };
      return self.inline_stage(&self.vir.stages[stage]);
    }

    let target = self.emit_interface(interface, false);

    self.pairs.push(match &interface.kind {
      InterfaceKind::Unconditional(stage) => (self.emit_stage_node(*stage), target),
      InterfaceKind::Branch(zero, non_zero) => (
        self.emit_port(transfer.data.as_ref().unwrap()),
        Tree::Branch(
          Box::new(self.emit_stage_node(*zero)),
          Box::new(self.emit_stage_node(*non_zero)),
          Box::new(target),
        ),
      ),
      InterfaceKind::Match(_, stages) => (
        self.emit_port(transfer.data.as_ref().unwrap()),
        Tree::n_ary("enum", stages.iter().map(|&s| self.emit_stage_node(s)).chain([target])),
      ),
      InterfaceKind::Fn { .. } => (self.emit_port(transfer.data.as_ref().unwrap()), target),
    });
  }

  fn finish_local(&mut self, mut local: LocalState) {
    if local.past.is_empty() {
      local.past.push((Vec::new(), Vec::new()));
    }
    let first = &mut local.past[0];
    first.0.append(&mut local.spaces);
    first.1.append(&mut local.values);
    for (mut spaces, mut values) in local.past.into_iter() {
      if spaces.is_empty() {
        for value in values {
          self.pairs.push((Tree::Erase, value));
        }
      } else if values.is_empty() {
        for space in spaces {
          self.pairs.push((space, Tree::Erase));
        }
      } else if values.len() == 1 {
        let label = self.dup_label();
        self.pairs.push((Tree::n_ary(&label, spaces), values.pop().unwrap()));
      } else if spaces.len() == 1 {
        let label = self.dup_label();
        self.pairs.push((spaces.pop().unwrap(), Tree::n_ary(&label, values)));
      } else {
        unreachable!()
      }
    }
  }

  fn inline_stage(&mut self, stage: &Stage) {
    let prev_wire_offset = self.wire_offset;
    self.wire_offset = self.wires.peek_next();
    self.wires.0 += stage.wires.0 .0;
    for local in &stage.declarations {
      if let Some(local) = self.locals.remove(local) {
        self.finish_local(local);
      }
    }
    for step in &stage.steps {
      self.emit_step(step);
    }
    self.wire_offset = prev_wire_offset;
  }

  fn _emit_stage(&mut self, stage: &Stage) {
    for local in &stage.declarations {
      if let Some(local) = self.locals.remove(local) {
        self.finish_local(local);
      }
    }
    for step in &stage.steps {
      self.emit_step(step);
    }
  }

  fn emit_interface(&mut self, interface: &Interface, side: bool) -> Tree {
    Tree::n_ary(
      "x",
      interface.wires.iter().filter_map(|(&local, usage)| {
        let usage = if side { usage.1 } else { usage.0 };
        match usage {
          Usage::Erase => {
            self.local(local).erase();
            None
          }
          Usage::Mut => {
            let a = self.new_wire();
            let b = self.new_wire();
            self.local(local).mutate(a.0, b.0);
            if side {
              Some(Tree::Comb("x".into(), Box::new(b.1), Box::new(a.1)))
            } else {
              Some(Tree::Comb("x".into(), Box::new(a.1), Box::new(b.1)))
            }
          }
          Usage::Set => {
            let w = self.new_wire();
            self.local(local).set(w.0);
            Some(w.1)
          }
          Usage::Take => {
            let w = self.new_wire();
            self.local(local).take(w.0);
            Some(w.1)
          }
          Usage::Get => {
            let w = self.new_wire();
            self.local(local).get(w.0);
            Some(w.1)
          }
          Usage::Hedge => {
            let w = self.new_wire();
            self.local(local).hedge(w.0);
            Some(w.1)
          }
          u => unreachable!("{u:?}"),
        }
      }),
    )
  }

  fn emit_stage_node(&self, stage_id: StageId) -> Tree {
    Tree::Global(self.stage_name(self.spec_id, stage_id))
  }

  fn local(&mut self, local: Local) -> &mut LocalState {
    self.locals.entry(local).or_default()
  }

  fn emit_step(&mut self, step: &Step) {
    let wire_offset = self.wire_offset;
    let emit_port = |p| Self::_emit_port(wire_offset, self.specs, self.fragments, self.spec, p);
    match step {
      Step::Invoke(local, invocation) => match invocation {
        Invocation::Erase => self.local(*local).erase(),
        Invocation::Get(port) => self.local(*local).get(emit_port(port)),
        Invocation::Hedge(port) => self.local(*local).hedge(emit_port(port)),
        Invocation::Take(port) => self.local(*local).take(emit_port(port)),
        Invocation::Set(port) => self.local(*local).set(emit_port(port)),
        Invocation::Mut(a, b) => self.local(*local).mutate(emit_port(a), emit_port(b)),
      },
      Step::Transfer(transfer) => self.emit_transfer(transfer),
      Step::Diverge(..) => unreachable!(),

      Step::Link(a, b) => self.pairs.push((emit_port(a), emit_port(b))),
      Step::Call(rel, recv, args, ret) => {
        let func = match self.spec.rels.fns[*rel] {
          Ok((spec_id, stage_id)) => Tree::Global(self.stage_name(spec_id, stage_id)),
          Err(_) => Tree::Erase,
        };
        self.pairs.push((
          func,
          Tree::n_ary("fn", [recv].into_iter().chain(args).chain([ret]).map(emit_port)),
        ))
      }
      Step::Composite(port, tuple) => {
        self.pairs.push((emit_port(port), Tree::n_ary("tup", tuple.iter().map(emit_port))))
      }
      Step::Enum(enum_id, variant_id, port, fields) => {
        let enum_def = &self.chart.enums[*enum_id];
        let fields = fields.iter().map(emit_port);
        let enum_ = make_enum(*variant_id, enum_def, || self.new_wire(), fields);
        self.pairs.push((emit_port(port), enum_));
      }
      Step::Ref(reference, value, space) => self.pairs.push((
        emit_port(reference),
        Tree::Comb("ref".into(), Box::new(emit_port(value)), Box::new(emit_port(space))),
      )),
      Step::ExtFn(ext_fn, swap, lhs, rhs, out) => self.pairs.push((
        emit_port(lhs),
        Tree::ExtFn(ext_fn.to_string(), *swap, Box::new(emit_port(rhs)), Box::new(emit_port(out))),
      )),
      Step::Dup(a, b, c) => {
        let label = self.dup_label();
        self
          .pairs
          .push((emit_port(a), Tree::Comb(label, Box::new(emit_port(b)), Box::new(emit_port(c)))))
      }
      Step::List(port, list) => {
        let str = self.emit_list(list.iter().map(emit_port));
        self.pairs.push((emit_port(port), str))
      }
      Step::String(port, init, rest) => {
        let const_len =
          init.chars().count() + rest.iter().map(|x| x.1.chars().count()).sum::<usize>();
        let len = self.new_wire();
        let start = self.new_wire();
        let end = self.new_wire();
        self.pairs.push((
          emit_port(port),
          Tree::n_ary(
            "tup",
            [
              len.0,
              Tree::n_ary("tup", init.chars().map(|c| Tree::N32(c as u32)).chain([start.0])),
              end.0,
            ],
          ),
        ));
        let mut cur_len = Tree::N32(const_len as u32);
        let mut cur_buf = start.1;
        for (port, seg) in rest {
          let next_len = self.new_wire();
          let next_buf = self.new_wire();
          self.pairs.push((
            emit_port(port),
            Tree::n_ary(
              "tup",
              [
                Tree::ExtFn("n32_add".into(), false, Box::new(cur_len), Box::new(next_len.0)),
                cur_buf,
                Tree::n_ary("tup", seg.chars().map(|c| Tree::N32(c as u32)).chain([next_buf.0])),
              ],
            ),
          ));
          cur_len = next_len.1;
          cur_buf = next_buf.1;
        }
        self.pairs.push((cur_len, len.1));
        self.pairs.push((cur_buf, end.1));
      }
      Step::InlineIvy(binds, out, net) => {
        for (var, port) in binds {
          self.pairs.push((Tree::Var(var.clone()), emit_port(port)));
        }
        self.pairs.push((emit_port(out), net.root.clone()));
        self.pairs.extend_from_slice(&net.pairs);
      }
    }
  }

  fn emit_port(&self, port: &Port) -> Tree {
    Self::_emit_port(self.wire_offset, self.specs, self.fragments, self.spec, port)
  }

  fn _emit_port(
    wire_offset: usize,
    specs: &Specializations,
    fragments: &IdxVec<FragmentId, Fragment<'core>>,
    spec: &Spec,
    port: &Port,
  ) -> Tree {
    match port {
      Port::Error(_) => Tree::Erase,
      Port::Erase => Tree::Erase,
      Port::N32(n) => Tree::N32(*n),
      Port::F32(f) => Tree::F32(*f),
      Port::Wire(w) => Tree::Var(format!("w{}", wire_offset + w.0)),
      Port::ConstRel(rel) => match spec.rels.consts[*rel] {
        Ok(spec_id) => Tree::Global(Self::_stage_name(specs, fragments, spec_id, StageId(0))),
        Err(_) => Tree::Erase,
      },
    }
  }

  fn emit_list(
    &mut self,
    ports: impl IntoIterator<IntoIter: DoubleEndedIterator<Item = Tree>>,
  ) -> Tree {
    let end = self.new_wire();
    let mut len = 0;
    let buf = Tree::n_ary("tup", ports.into_iter().inspect(|_| len += 1).chain([end.0]));
    Tree::n_ary("tup", [Tree::N32(len), buf, end.1])
  }

  fn emit_header(&self, header: &Header, root: Tree) -> Tree {
    match header {
      Header::None => root,
      Header::Match(None) => root,
      Header::Match(Some(data)) => {
        Tree::Comb("enum".into(), Box::new(self.emit_port(data)), Box::new(root))
      }
      Header::Fn(params, result) => Tree::n_ary(
        "fn",
        [root].into_iter().chain(params.iter().chain([result]).map(|port| self.emit_port(port))),
      ),
      Header::Fork(former, latter) => Tree::n_ary(
        "fn",
        [
          Tree::Erase,
          Tree::Comb("ref".into(), Box::new(root), Box::new(self.emit_port(latter))),
          self.emit_port(former),
        ],
      ),
      Header::Drop => Tree::n_ary("fn", [Tree::Erase, root, Tree::Erase]),
    }
  }

  fn new_wire(&mut self) -> (Tree, Tree) {
    let label = format!("w{}", self.wires.next());
    (Tree::Var(label.clone()), Tree::Var(label))
  }

  pub fn stage_name(&self, spec_id: SpecId, stage_id: StageId) -> String {
    Self::_stage_name(self.specs, self.fragments, spec_id, stage_id)
  }

  fn _stage_name(
    specs: &Specializations,
    fragments: &IdxVec<FragmentId, Fragment<'core>>,
    spec_id: SpecId,
    stage_id: StageId,
  ) -> String {
    let spec = specs.specs[spec_id].as_ref().unwrap();
    let path = fragments[spec.fragment].path;
    let mut name = path.to_owned();
    if !spec.singular {
      write!(name, "::{}", spec.index).unwrap();
    }
    if stage_id.0 != 0 {
      write!(name, "::{}", stage_id.0).unwrap();
    }
    name
  }
}

#[derive(Default)]
struct LocalState {
  past: Vec<(Vec<Tree>, Vec<Tree>)>,
  spaces: Vec<Tree>,
  values: Vec<Tree>,
}

impl LocalState {
  fn mutate(&mut self, a: Tree, b: Tree) {
    self.get(a);
    self.erase();
    self.hedge(b);
  }

  fn get(&mut self, port: Tree) {
    self.spaces.push(port);
  }

  fn take(&mut self, port: Tree) {
    self.get(port);
    self.erase();
  }

  fn hedge(&mut self, port: Tree) {
    self.values.push(port);
  }

  fn set(&mut self, port: Tree) {
    self.erase();
    self.hedge(port);
  }

  fn erase(&mut self) {
    if self.past.is_empty() || !self.spaces.is_empty() || !self.values.is_empty() {
      self.past.push((take(&mut self.spaces), take(&mut self.values)));
    }
  }
}

fn make_enum(
  variant_id: VariantId,
  enum_def: &EnumDef,
  mut new_wire: impl FnMut() -> (Tree, Tree),
  fields: impl DoubleEndedIterator<Item = Tree>,
) -> Tree {
  let wire = new_wire();
  let mut fields = Tree::n_ary("enum", fields.chain([wire.0]));
  Tree::n_ary(
    "enum",
    (0..enum_def.variants.len())
      .map(|i| if variant_id.0 == i { take(&mut fields) } else { Tree::Erase })
      .chain([wire.1]),
  )
}
