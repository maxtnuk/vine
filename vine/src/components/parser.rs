mod parse_string;

use std::{mem::transmute, path::PathBuf};

use ivy::parser::IvyParser;
use vine_util::parser::{Parser, ParserState};

use crate::{
  components::lexer::Token,
  structures::{core::Core, diag::Diag},
};

use crate::structures::ast::{
  Attr, AttrKind, BinaryOp, Block, Builtin, ComparisonOp, ConstItem, EnumItem, Expr, ExprKind,
  Flex, FnItem, GenericArgs, GenericParams, Generics, Ident, Impl, ImplItem, ImplKind, ImplParam,
  Item, ItemKind, Key, LetFnStmt, LetStmt, LogicalOp, ModItem, ModKind, Pat, PatKind, Path, Span,
  Stmt, StmtKind, StructItem, Trait, TraitItem, TraitKind, Ty, TyKind, TypeItem, TypeParam,
  UseItem, UseTree, Variant, Vis,
};

pub struct VineParser<'core, 'src> {
  pub(crate) core: &'core Core<'core>,
  pub(crate) state: ParserState<'src, Token>,
  pub(crate) file: usize,
}

impl<'core, 'src> Parser<'src> for VineParser<'core, 'src> {
  type Token = Token;
  type Error = Diag<'core>;

  fn state(&mut self) -> &mut ParserState<'src, Self::Token> {
    &mut self.state
  }

  fn lex_error(&self) -> Self::Error {
    Diag::LexError { span: self.span() }
  }

  fn unexpected_error(&self) -> Diag<'core> {
    Diag::UnexpectedToken {
      span: self.span(),
      expected: self.state.expected,
      found: self.state.token,
    }
  }
}

type Parse<'core, T = ()> = Result<T, Diag<'core>>;

impl<'core, 'src> VineParser<'core, 'src> {
  pub fn parse(
    core: &'core Core<'core>,
    src: &'src str,
    file: usize,
  ) -> Parse<'core, Vec<Item<'core>>> {
    let mut parser = VineParser { core, state: ParserState::new(src), file };
    parser.bump()?;
    let mut items = Vec::new();
    while parser.state.token.is_some() {
      items.push(parser.parse_item()?);
    }
    Ok(items)
  }

  fn parse_item(&mut self) -> Parse<'core, Item<'core>> {
    self.maybe_parse_item()?.ok_or_else(|| self.unexpected_error())
  }

  fn maybe_parse_item(&mut self) -> Parse<'core, Option<Item<'core>>> {
    let span = self.start_span();
    let mut attrs = Vec::new();
    while self.check(Token::Hash) {
      attrs.push(self.parse_attr()?);
    }
    let vis = self.parse_vis()?;
    let kind = match () {
      _ if self.check(Token::Fn) => ItemKind::Fn(self.parse_fn_item()?),
      _ if self.check(Token::Const) => ItemKind::Const(self.parse_const_item()?),
      _ if self.check(Token::Struct) => ItemKind::Struct(self.parse_struct_item()?),
      _ if self.check(Token::Enum) => ItemKind::Enum(self.parse_enum_item()?),
      _ if self.check(Token::Type) => ItemKind::Type(self.parse_type_item()?),
      _ if self.check(Token::Mod) => ItemKind::Mod(self.parse_mod_item()?),
      _ if self.check(Token::Trait) => ItemKind::Trait(self.parse_trait_item()?),
      _ if self.check(Token::Impl) => ItemKind::Impl(self.parse_impl_item()?),
      _ if self.check(Token::Use) => ItemKind::Use(self.parse_use_item()?),
      _ if span == self.start_span() => return Ok(None),
      _ => self.unexpected()?,
    };
    let span = self.end_span(span);
    Ok(Some(Item { vis, span, attrs, kind }))
  }

  fn parse_vis(&mut self) -> Parse<'core, Vis<'core>> {
    Ok(if self.eat(Token::Pub)? {
      if self.eat(Token::Dot)? {
        let span = self.start_span();
        let ancestor = self.parse_ident()?;
        let span = self.end_span(span);
        Vis::PublicTo(span, ancestor)
      } else {
        Vis::Public
      }
    } else {
      Vis::Private
    })
  }

  fn parse_attr(&mut self) -> Parse<'core, Attr> {
    let span = self.start_span();
    self.expect(Token::Hash)?;
    self.expect(Token::OpenBracket)?;
    let ident_span = self.start_span();
    let ident = self.expect(Token::Ident)?;
    let ident_span = self.end_span(ident_span);
    let kind = match ident {
      "builtin" => {
        self.expect(Token::Eq)?;
        let str_span = self.start_span();
        let str = self.parse_string()?;
        let str_span = self.end_span(str_span);
        let builtin = match &*str {
          "Bool" => Builtin::Bool,
          "N32" => Builtin::N32,
          "I32" => Builtin::I32,
          "F32" => Builtin::F32,
          "Char" => Builtin::Char,
          "IO" => Builtin::IO,
          "List" => Builtin::List,
          "String" => Builtin::String,
          "Result" => Builtin::Result,
          "prelude" => Builtin::Prelude,
          "neg" => Builtin::Neg,
          "not" => Builtin::Not,
          "bool_not" => Builtin::BoolNot,
          "cast" => Builtin::Cast,
          "add" => Builtin::BinaryOp(BinaryOp::Add),
          "sub" => Builtin::BinaryOp(BinaryOp::Sub),
          "mul" => Builtin::BinaryOp(BinaryOp::Mul),
          "div" => Builtin::BinaryOp(BinaryOp::Div),
          "rem" => Builtin::BinaryOp(BinaryOp::Rem),
          "and" => Builtin::BinaryOp(BinaryOp::BitAnd),
          "or" => Builtin::BinaryOp(BinaryOp::BitOr),
          "xor" => Builtin::BinaryOp(BinaryOp::BitXor),
          "shl" => Builtin::BinaryOp(BinaryOp::Shl),
          "shr" => Builtin::BinaryOp(BinaryOp::Shr),
          "concat" => Builtin::BinaryOp(BinaryOp::Concat),
          "pow" => Builtin::BinaryOp(BinaryOp::Pow),
          "eq" => Builtin::ComparisonOp(ComparisonOp::Eq),
          "ne" => Builtin::ComparisonOp(ComparisonOp::Ne),
          "lt" => Builtin::ComparisonOp(ComparisonOp::Lt),
          "gt" => Builtin::ComparisonOp(ComparisonOp::Gt),
          "le" => Builtin::ComparisonOp(ComparisonOp::Le),
          "ge" => Builtin::ComparisonOp(ComparisonOp::Ge),
          "Fork" => Builtin::Fork,
          "Drop" => Builtin::Drop,
          "copy" => Builtin::Copy,
          "erase" => Builtin::Erase,
          "Range" => Builtin::Range,
          "BoundUnbounded" => Builtin::BoundUnbounded,
          "BoundInclusive" => Builtin::BoundInclusive,
          "BoundExclusive" => Builtin::BoundExclusive,
          _ => Err(Diag::BadBuiltin { span: str_span })?,
        };
        AttrKind::Builtin(builtin)
      }
      _ => Err(Diag::UnknownAttribute { span: ident_span })?,
    };
    self.expect(Token::CloseBracket)?;
    let span = self.end_span(span);
    Ok(Attr { span, kind })
  }

  fn parse_ident(&mut self) -> Parse<'core, Ident<'core>> {
    let token = self.expect(Token::Ident)?;
    Ok(self.core.ident(token))
  }

  fn parse_key(&mut self) -> Parse<'core, Key<'core>> {
    let span = self.start_span();
    let ident = self.parse_ident()?;
    let span = self.end_span(span);
    Ok(Key { span, ident })
  }

  fn parse_num(&mut self) -> Parse<'core, ExprKind<'core>> {
    let span = self.span();
    let token = self.expect(Token::Num)?;
    if token.contains('.') {
      Ok(ExprKind::F32(self.parse_f32_like(token, |_| Diag::InvalidNum { span })?))
    } else if token.starts_with("+") || token.starts_with("-") {
      let abs = self.parse_u32_like(&token[1..], |_| Diag::InvalidNum { span })? as i32;
      let num = if token.starts_with("-") { -abs } else { abs };
      Ok(ExprKind::I32(num))
    } else {
      Ok(ExprKind::N32(self.parse_u32_like(token, |_| Diag::InvalidNum { span })?))
    }
  }

  fn parse_fn_item(&mut self) -> Parse<'core, FnItem<'core>> {
    self.expect(Token::Fn)?;
    let method = self.eat(Token::Dot)?;
    let name = self.parse_ident()?;
    let generics = self.parse_generic_params()?;
    let params = self.parse_delimited(PAREN_COMMA, Self::parse_pat)?;
    let ret = self.eat(Token::ThinArrow)?.then(|| self.parse_type()).transpose()?;
    let body = (!self.eat(Token::Semi)?).then(|| self.parse_block()).transpose()?;
    Ok(FnItem { method, name, generics, params, ret, body })
  }

  fn parse_const_item(&mut self) -> Parse<'core, ConstItem<'core>> {
    self.expect(Token::Const)?;
    let name = self.parse_ident()?;
    let generics = self.parse_generic_params()?;
    self.expect(Token::Colon)?;
    let ty = self.parse_type()?;
    let value = self.eat(Token::Eq)?.then(|| self.parse_expr()).transpose()?;
    self.expect(Token::Semi)?;
    Ok(ConstItem { name, generics, ty, value })
  }

  fn parse_struct_item(&mut self) -> Parse<'core, StructItem<'core>> {
    self.expect(Token::Struct)?;
    let name = self.parse_ident()?;
    let generics = self.parse_generic_params()?;
    self.expect(Token::OpenParen)?;
    let data_vis = self.parse_vis()?;
    let data = self.parse_type()?;
    self.expect(Token::CloseParen)?;
    self.eat(Token::Semi)?;
    Ok(StructItem { name, generics, data_vis, data })
  }

  fn parse_enum_item(&mut self) -> Parse<'core, EnumItem<'core>> {
    self.expect(Token::Enum)?;
    let name = self.parse_ident()?;
    let generics = self.parse_generic_params()?;
    let variants = self.parse_delimited(BRACE_COMMA, Self::parse_variant)?;
    Ok(EnumItem { name, generics, variants })
  }

  fn parse_type_item(&mut self) -> Parse<'core, TypeItem<'core>> {
    self.expect(Token::Type)?;
    let name = self.parse_ident()?;
    let generics = self.parse_generic_params()?;
    let ty = self.eat(Token::Eq)?.then(|| self.parse_type()).transpose()?;
    self.eat(Token::Semi)?;
    Ok(TypeItem { name, generics, ty })
  }

  fn parse_variant(&mut self) -> Parse<'core, Variant<'core>> {
    let name = self.parse_ident()?;
    let data = if self.eat(Token::OpenParen)? {
      let ty = self.parse_type()?;
      self.eat(Token::CloseParen)?;
      Some(ty)
    } else {
      None
    };
    Ok(Variant { name, data })
  }

  fn parse_generic_params(&mut self) -> Parse<'core, GenericParams<'core>> {
    self.parse_generics(Self::parse_type_param, Self::parse_impl_param)
  }

  fn parse_type_param(&mut self) -> Parse<'core, TypeParam<'core>> {
    let span = self.start_span();
    let name = self.parse_ident()?;
    let flex = self.parse_flex()?;
    let span = self.end_span(span);
    Ok(TypeParam { span, name, flex })
  }

  fn parse_impl_param(&mut self) -> Parse<'core, ImplParam<'core>> {
    let span = self.start_span();
    let (name, trait_) = if self.check(Token::Ident) {
      let path = self.parse_path()?;
      if path.as_ident().is_some() && self.eat(Token::Colon)? {
        let trait_ = self.parse_trait()?;
        (path.as_ident(), trait_)
      } else {
        (None, Trait { span: path.span, kind: TraitKind::Path(path) })
      }
    } else {
      (None, self.parse_trait()?)
    };
    let span = self.end_span(span);
    Ok(ImplParam { span, name, trait_ })
  }

  fn parse_flex(&mut self) -> Parse<'core, Flex> {
    if self.eat(Token::Plus)? {
      Ok(Flex::Fork)
    } else if self.eat(Token::Question)? {
      Ok(Flex::Drop)
    } else if self.eat(Token::Star)? {
      Ok(Flex::Full)
    } else {
      Ok(Flex::None)
    }
  }

  fn parse_generic_args(&mut self) -> Parse<'core, GenericArgs<'core>> {
    self.parse_generics(Self::parse_type, Self::parse_impl)
  }

  fn parse_generics<T, I>(
    &mut self,
    mut parse_t: impl FnMut(&mut Self) -> Parse<'core, T>,
    mut parse_i: impl FnMut(&mut Self) -> Parse<'core, I>,
  ) -> Parse<'core, Generics<T, I>> {
    let span = self.start_span();
    let mut types = Vec::new();
    let mut impls = Vec::new();
    if self.eat(Token::OpenBracket)? {
      loop {
        if self.eat(Token::Semi)? || self.check(Token::CloseBracket) {
          break;
        }
        types.push(parse_t(self)?);
        if self.eat(Token::Comma)? {
          continue;
        }
        if self.eat(Token::Semi)? {
          break;
        }
        if !self.check(Token::CloseBracket) {
          self.unexpected()?;
        }
      }
      loop {
        if self.eat(Token::CloseBracket)? {
          break;
        }
        impls.push(parse_i(self)?);
        if self.eat(Token::Comma)? {
          continue;
        }
        self.expect(Token::CloseBracket)?;
        break;
      }
    }
    let span = self.end_span(span);
    Ok(Generics { span, types, impls })
  }

  fn parse_mod_item(&mut self) -> Parse<'core, ModItem<'core>> {
    self.expect(Token::Mod)?;
    let name = self.parse_ident()?;
    if self.eat(Token::Eq)? {
      let span = self.start_span();
      let path = self.parse_string()?;
      let span = self.end_span(span);
      self.expect(Token::Semi)?;
      Ok(ModItem { name, kind: ModKind::Unloaded(span, PathBuf::from(path)) })
    } else {
      let span = self.start_span();
      let items = self.parse_delimited(BRACE, Self::parse_item)?;
      let span = self.end_span(span);
      Ok(ModItem { name, kind: ModKind::Loaded(span, items) })
    }
  }

  fn parse_trait_item(&mut self) -> Parse<'core, TraitItem<'core>> {
    self.expect(Token::Trait)?;
    let name = self.parse_ident()?;
    let generics = self.parse_generic_params()?;
    let items = self.parse_delimited(BRACE, Self::parse_item)?;
    Ok(TraitItem { name, generics, items })
  }

  fn parse_impl_item(&mut self) -> Parse<'core, ImplItem<'core>> {
    self.expect(Token::Impl)?;
    let name = self.parse_ident()?;
    let generics = self.parse_generic_params()?;
    self.expect(Token::Colon)?;
    let trait_ = self.parse_trait()?;
    let items = self.parse_delimited(BRACE, Self::parse_item)?;
    Ok(ImplItem { name, generics, trait_, items })
  }

  fn parse_use_item(&mut self) -> Parse<'core, UseItem<'core>> {
    self.expect(Token::Use)?;
    let absolute = self.eat(Token::ColonColon)?;
    let span = self.start_span();
    let mut tree = UseTree::empty(self.span());
    loop {
      self.parse_use_tree(None, &mut tree)?;
      if !self.eat(Token::Comma)? {
        break;
      }
    }
    tree.prune();
    tree.span = self.end_span(span);
    self.eat(Token::Semi)?;
    Ok(UseItem { absolute, tree })
  }

  fn parse_use_tree(
    &mut self,
    cur_name: Option<Ident<'core>>,
    tree: &mut UseTree<'core>,
  ) -> Parse<'core> {
    if self.check(Token::Ident) {
      let span = self.span();
      let ident = self.parse_ident()?;
      if cur_name == Some(ident) {
        if self.eat(Token::As)? {
          let alias = self.parse_ident()?;
          tree.aliases.push(alias);
        } else {
          tree.aliases.push(ident);
        }
      } else {
        let child = tree.children.entry(ident).or_insert(UseTree::empty(span));
        if self.eat(Token::ColonColon)? {
          let is_group = self.check(Token::OpenBrace);
          self.parse_use_tree(is_group.then_some(ident), child)?;
        } else if self.eat(Token::As)? {
          let alias = self.parse_ident()?;
          child.aliases.push(alias);
        } else {
          child.aliases.push(ident);
        }
      }
    } else {
      self.parse_delimited(BRACE_COMMA, |self_| self_.parse_use_tree(cur_name, tree))?;
    }
    Ok(())
  }

  fn parse_expr_list(&mut self) -> Parse<'core, Vec<Expr<'core>>> {
    self.parse_delimited(PAREN_COMMA, Self::parse_expr)
  }

  fn parse_block(&mut self) -> Parse<'core, Block<'core>> {
    let span = self.start_span();
    let stmts = self.parse_delimited(BRACE, Self::parse_stmt)?;
    let span = self.end_span(span);
    Ok(Block { span, stmts })
  }

  fn parse_expr(&mut self) -> Parse<'core, Expr<'core>> {
    self.parse_expr_bp(BP::Min)
  }

  fn maybe_parse_expr_bp(&mut self, bp: BP) -> Parse<'core, Option<Expr<'core>>> {
    let span = self.start_span();
    let Some(mut expr) = self.maybe_parse_expr_prefix()? else {
      return Ok(None);
    };
    loop {
      expr = match self.parse_expr_postfix(expr, bp)? {
        Ok(kind) => Expr { span: self.end_span(span), kind },
        Err(expr) => return Ok(Some(expr)),
      }
    }
  }

  fn parse_expr_bp(&mut self, bp: BP) -> Parse<'core, Expr<'core>> {
    match self.maybe_parse_expr_bp(bp)? {
      Some(expr) => Ok(expr),
      None => self.unexpected(),
    }
  }

  fn maybe_parse_expr_prefix(&mut self) -> Parse<'core, Option<Expr<'core>>> {
    let span = self.start_span();
    let Some(kind) = self._maybe_parse_expr_prefix(span)? else {
      return Ok(None);
    };
    let span = self.end_span(span);
    Ok(Some(Expr { span, kind }))
  }

  fn _maybe_parse_expr_prefix(&mut self, span: usize) -> Parse<'core, Option<ExprKind<'core>>> {
    if self.eat(Token::Return)? {
      if self.check(Token::Semi) {
        return Ok(Some(ExprKind::Return(None)));
      }

      return Ok(Some(ExprKind::Return(Some(Box::new(self.parse_expr_bp(BP::ControlFlow)?)))));
    }
    if self.eat(Token::Break)? {
      let label = self.parse_label()?;

      if self.check(Token::Semi) {
        return Ok(Some(ExprKind::Break(label, None)));
      }

      return Ok(Some(ExprKind::Break(
        label,
        Some(Box::new(self.parse_expr_bp(BP::ControlFlow)?)),
      )));
    }
    if self.eat(Token::Continue)? {
      return Ok(Some(ExprKind::Continue(self.parse_label()?)));
    }
    if self.eat(Token::And)? {
      return Ok(Some(ExprKind::Ref(Box::new(self.parse_expr_bp(BP::Prefix)?), false)));
    }
    if self.eat(Token::AndAnd)? {
      let inner = self.parse_expr_bp(BP::Prefix)?;
      let span = self.end_span(span + 1);
      return Ok(Some(ExprKind::Ref(
        Box::new(Expr { span, kind: ExprKind::Ref(Box::new(inner), false) }),
        false,
      )));
    }
    if self.eat(Token::Star)? {
      return Ok(Some(ExprKind::Deref(Box::new(self.parse_expr_bp(BP::Prefix)?), false)));
    }
    if self.eat(Token::Move)? {
      return Ok(Some(ExprKind::Move(Box::new(self.parse_expr_bp(BP::Prefix)?), false)));
    }
    if self.eat(Token::Tilde)? {
      return Ok(Some(ExprKind::Inverse(Box::new(self.parse_expr_bp(BP::Prefix)?), false)));
    }
    if self.eat(Token::Minus)? {
      return Ok(Some(ExprKind::Neg(Box::new(self.parse_expr_bp(BP::Prefix)?))));
    }
    if self.eat(Token::Bang)? {
      return Ok(Some(ExprKind::Not(Box::new(self.parse_expr_bp(BP::Prefix)?))));
    }
    if self.eat(Token::DotDot)? {
      let right_bound = self.maybe_parse_expr_bp(BP::Range)?;
      return Ok(Some(ExprKind::RangeExclusive(None, right_bound.map(Box::new))));
    }
    if self.eat(Token::DotDotEq)? {
      let right_bound = self.parse_expr_bp(BP::Range)?;
      return Ok(Some(ExprKind::RangeInclusive(None, Box::new(right_bound))));
    }
    if self.check(Token::Num) {
      return Ok(Some(self.parse_num()?));
    }
    if self.check(Token::SingleQuote) {
      return Ok(Some(self.parse_char_expr()?));
    }
    if self.check(Token::DoubleQuote) {
      return Ok(Some(self.parse_string_expr()?));
    }
    if self.eat(Token::True)? {
      return Ok(Some(ExprKind::Bool(true)));
    }
    if self.eat(Token::False)? {
      return Ok(Some(ExprKind::Bool(false)));
    }
    if self.check(Token::Ident) || self.check(Token::ColonColon) {
      let path = self.parse_path()?;
      let args = self.check(Token::OpenParen).then(|| self.parse_expr_list()).transpose()?;
      return Ok(Some(ExprKind::Path(path, args)));
    }
    if self.eat(Token::OpenParen)? {
      if self.eat(Token::CloseParen)? {
        return Ok(Some(ExprKind::Tuple(vec![])));
      }
      let expr = self.parse_expr()?;
      if self.eat(Token::Semi)? {
        let value = expr;
        let space = self.parse_expr()?;
        self.expect(Token::CloseParen)?;
        return Ok(Some(ExprKind::Place(Box::new(value), Box::new(space))));
      }
      if self.eat(Token::CloseParen)? {
        return Ok(Some(ExprKind::Paren(Box::new(expr))));
      }
      self.expect(Token::Comma)?;
      let mut exprs = vec![expr];
      loop {
        if self.check(Token::CloseParen) {
          break;
        }
        exprs.push(self.parse_expr()?);
        if !self.eat(Token::Comma)? {
          break;
        }
      }
      self.expect(Token::CloseParen)?;
      return Ok(Some(ExprKind::Tuple(exprs)));
    }
    if self.check(Token::OpenBrace) {
      return Ok(Some(ExprKind::Object(self.parse_delimited(BRACE_COMMA, |self_| {
        let key = self_.parse_key()?;
        let value = if self_.eat(Token::Colon)? {
          self_.parse_expr()?
        } else {
          Expr { span: key.span, kind: ExprKind::Path(key.into(), None) }
        };
        Ok((key, value))
      })?)));
    }
    if self.check(Token::OpenBracket) {
      let exprs = self.parse_delimited(BRACKET_COMMA, Self::parse_expr)?;
      return Ok(Some(ExprKind::List(exprs)));
    }
    if self.eat(Token::Do)? {
      return Ok(Some(ExprKind::Do(self.parse_label()?, self.parse_block()?)));
    }
    if self.eat(Token::If)? {
      let mut arms = Vec::new();
      loop {
        let cond = self.parse_expr()?;
        let then = self.parse_block()?;
        arms.push((cond, then));
        if self.eat(Token::Else)? {
          if self.eat(Token::If)? {
            continue;
          } else {
            let leg = self.parse_block()?;
            return Ok(Some(ExprKind::If(arms, Some(leg))));
          }
        } else {
          return Ok(Some(ExprKind::If(arms, None)));
        }
      }
    }
    if self.eat(Token::While)? {
      let label = self.parse_label()?;
      let cond = self.parse_expr()?;
      let body = self.parse_block()?;
      return Ok(Some(ExprKind::While(label, Box::new(cond), body)));
    }
    if self.eat(Token::Loop)? {
      let label = self.parse_label()?;
      let body = self.parse_block()?;
      return Ok(Some(ExprKind::Loop(label, body)));
    }
    if self.eat(Token::Fn)? {
      let flex = self.parse_flex()?;
      let params = self.parse_delimited(PAREN_COMMA, Self::parse_pat)?;
      let body = self.parse_block()?;
      return Ok(Some(ExprKind::Fn(flex, params, None, body)));
    }
    if self.eat(Token::Match)? {
      let scrutinee = self.parse_expr()?;
      let arms = self.parse_delimited(BRACE, |self_| {
        let pat = self_.parse_pat()?;
        let value = self_.parse_block()?;
        Ok((pat, value))
      })?;
      return Ok(Some(ExprKind::Match(Box::new(scrutinee), arms)));
    }
    if self.eat(Token::Hole)? {
      return Ok(Some(ExprKind::Hole));
    }
    if self.check(Token::InlineIvy) {
      let span = self.span();
      self.bump()?;
      let binds = self.parse_delimited(PAREN_COMMA, |self_| {
        let var = self_.parse_ident()?;
        let value = self_.eat(Token::ThinLeftArrow)?;
        if !value {
          self_.expect(Token::ThinArrow)?;
        }
        let expr = self_.parse_expr()?;
        Ok((var, value, expr))
      })?;
      self.expect(Token::ThinArrow)?;
      let ty = self.parse_type()?;
      if !self.check(Token::OpenBrace) {
        self.unexpected()?;
      }
      let net_span = self.start_span();
      let net = self.switch(
        |state| IvyParser { state },
        IvyParser::parse_net_inner,
        |_| Diag::InvalidIvy { span },
      )?;
      let net_span = self.end_span(net_span);
      return Ok(Some(ExprKind::InlineIvy(binds, ty, net_span, net)));
    }
    Ok(None)
  }

  fn parse_expr_postfix(
    &mut self,
    lhs: Expr<'core>,
    bp: BP,
  ) -> Parse<'core, Result<ExprKind<'core>, Expr<'core>>> {
    for &(lbp, associativity, token, op) in BINARY_OP_TABLE {
      let rbp = match associativity {
        Associativity::Left => lbp.inc(),
        Associativity::Right => lbp,
      };
      if bp.permits(lbp) && self.eat(token)? {
        if self.eat(Token::Eq)? {
          return Ok(Ok(ExprKind::BinaryOpAssign(
            op,
            Box::new(lhs),
            Box::new(self.parse_expr_bp(BP::Assignment)?),
          )));
        } else {
          return Ok(Ok(ExprKind::BinaryOp(op, Box::new(lhs), Box::new(self.parse_expr_bp(rbp)?))));
        }
      }
    }

    if bp.permits(BP::LogicalAnd) && self.eat(Token::AndAnd)? {
      let rhs = self.parse_expr_bp(BP::LogicalAnd)?;
      return Ok(Ok(ExprKind::LogicalOp(LogicalOp::And, Box::new(lhs), Box::new(rhs))));
    }

    if bp.permits(BP::LogicalOr) && self.eat(Token::OrOr)? {
      let rhs = self.parse_expr_bp(BP::LogicalOr)?;
      return Ok(Ok(ExprKind::LogicalOp(LogicalOp::Or, Box::new(lhs), Box::new(rhs))));
    }

    if bp.permits(BP::LogicalImplies) && self.eat(Token::ThickArrow)? {
      let rhs = self.parse_expr_bp(BP::LogicalImplies)?;
      return Ok(Ok(ExprKind::LogicalOp(LogicalOp::Implies, Box::new(lhs), Box::new(rhs))));
    }

    if bp.permits(BP::Is) && self.eat(Token::Is)? {
      let rhs = self.parse_pat()?;
      return Ok(Ok(ExprKind::Is(Box::new(lhs), Box::new(rhs))));
    }

    if bp.permits(BP::Comparison) {
      let mut rhs = Vec::new();
      'main: loop {
        for &(token, op) in COMPARISON_OP_TABLE {
          if self.eat(token)? {
            rhs.push((op, self.parse_expr_bp(BP::Comparison.inc())?));
            continue 'main;
          }
        }
        break;
      }
      if !rhs.is_empty() {
        return Ok(Ok(ExprKind::ComparisonOp(Box::new(lhs), rhs)));
      }
    }

    if bp.permits(BP::Assignment) && self.eat(Token::Eq)? {
      let rhs = self.parse_expr_bp(BP::Assignment)?;
      return Ok(Ok(ExprKind::Assign(false, Box::new(lhs), Box::new(rhs))));
    }

    if bp.permits(BP::Assignment) && self.eat(Token::Tilde)? {
      self.expect(Token::Eq)?;
      let rhs = self.parse_expr_bp(BP::Assignment)?;
      return Ok(Ok(ExprKind::Assign(true, Box::new(lhs), Box::new(rhs))));
    }

    if bp.permits(BP::Annotation) && self.eat(Token::As)? {
      let ty = self.parse_type()?;
      return Ok(Ok(ExprKind::Cast(Box::new(lhs), Box::new(ty), false)));
    }

    if self.eat(Token::Bang)? {
      return Ok(Ok(ExprKind::Unwrap(Box::new(lhs))));
    }

    if self.eat(Token::Question)? {
      return Ok(Ok(ExprKind::Try(Box::new(lhs))));
    }

    if bp.permits(BP::Range) && self.eat(Token::DotDot)? {
      let rhs = self.maybe_parse_expr_bp(BP::Range)?;
      return Ok(Ok(ExprKind::RangeExclusive(Some(Box::new(lhs)), rhs.map(Box::new))));
    }

    if bp.permits(BP::Range) && self.eat(Token::DotDotEq)? {
      let rhs = self.parse_expr_bp(BP::Range)?;
      return Ok(Ok(ExprKind::RangeInclusive(Some(Box::new(lhs)), Box::new(rhs))));
    }

    if self.eat(Token::Dot)? {
      if self.eat(Token::And)? {
        return Ok(Ok(ExprKind::Ref(Box::new(lhs), true)));
      }
      if self.eat(Token::Star)? {
        return Ok(Ok(ExprKind::Deref(Box::new(lhs), true)));
      }
      if self.eat(Token::Move)? {
        return Ok(Ok(ExprKind::Move(Box::new(lhs), true)));
      }
      if self.eat(Token::Tilde)? {
        return Ok(Ok(ExprKind::Inverse(Box::new(lhs), true)));
      }
      if self.eat(Token::As)? {
        self.expect(Token::OpenBracket)?;
        let ty = self.parse_type()?;
        self.expect(Token::CloseBracket)?;
        return Ok(Ok(ExprKind::Cast(Box::new(lhs), Box::new(ty), true)));
      }
      if self.check(Token::Num) {
        let token_span = self.start_span();
        let num = self.expect(Token::Num)?;
        let token_span = self.end_span(token_span);
        if let Some((i, j)) = num.split_once(".") {
          let i_span =
            Span { file: self.file, start: token_span.start, end: token_span.start + i.len() };
          let j_span =
            Span { file: self.file, start: token_span.end - j.len(), end: token_span.end };
          let i = self.parse_u32_like(i, |_| Diag::InvalidNum { span: i_span })? as usize;
          let j = self.parse_u32_like(j, |_| Diag::InvalidNum { span: j_span })? as usize;
          return Ok(Ok(ExprKind::TupleField(
            Box::new(Expr {
              span: Span { file: self.file, start: lhs.span.start, end: i_span.end },
              kind: ExprKind::TupleField(Box::new(lhs), i, None),
            }),
            j,
            None,
          )));
        } else {
          let i = self.parse_u32_like(num, |_| Diag::InvalidNum { span: token_span })? as usize;
          return Ok(Ok(ExprKind::TupleField(Box::new(lhs), i, None)));
        }
      }
      let key = self.parse_key()?;
      if self.check(Token::OpenBracket) || self.check(Token::OpenParen) {
        let generics = self.parse_generic_args()?;
        let args = self.parse_expr_list()?;
        return Ok(Ok(ExprKind::Method(Box::new(lhs), key.ident, generics, args)));
      } else {
        return Ok(Ok(ExprKind::ObjectField(Box::new(lhs), key)));
      }
    }

    if self.check(Token::OpenParen) {
      let args = self.parse_expr_list()?;
      return Ok(Ok(ExprKind::Call(Box::new(lhs), args)));
    }

    Ok(Err(lhs))
  }

  fn parse_label(&mut self) -> Parse<'core, Option<Ident<'core>>> {
    self.eat(Token::Dot)?.then(|| self.parse_ident()).transpose()
  }

  fn parse_pat(&mut self) -> Parse<'core, Pat<'core>> {
    self.parse_pat_bp(BP::Min)
  }

  fn parse_pat_bp(&mut self, bp: BP) -> Parse<'core, Pat<'core>> {
    let span = self.start_span();
    let mut pat = self.parse_pat_prefix()?;
    loop {
      pat = match self.parse_pat_postfix(pat, bp)? {
        Ok(kind) => Pat { span: self.end_span(span), kind },
        Err(pat) => return Ok(pat),
      }
    }
  }

  fn parse_pat_prefix(&mut self) -> Parse<'core, Pat<'core>> {
    let span = self.start_span();
    let kind = self._parse_pat_prefix(span)?;
    let span = self.end_span(span);
    Ok(Pat { span, kind })
  }

  fn _parse_pat_prefix(&mut self, span: usize) -> Parse<'core, PatKind<'core>> {
    if self.eat(Token::Hole)? {
      return Ok(PatKind::Hole);
    }
    if self.check(Token::Ident) || self.check(Token::ColonColon) {
      let path = self.parse_path()?;
      let data = self
        .check(Token::OpenParen)
        .then(|| self.parse_delimited(PAREN_COMMA, Self::parse_pat))
        .transpose()?;
      return Ok(PatKind::Path(path, data));
    }
    if self.eat(Token::And)? {
      return Ok(PatKind::Ref(Box::new(self.parse_pat_bp(BP::Prefix)?)));
    }
    if self.eat(Token::AndAnd)? {
      let inner = self.parse_pat_bp(BP::Prefix)?;
      let span = self.end_span(span + 1);
      return Ok(PatKind::Ref(Box::new(Pat { span, kind: PatKind::Ref(Box::new(inner)) })));
    }
    if self.eat(Token::Star)? {
      return Ok(PatKind::Deref(Box::new(self.parse_pat_bp(BP::Prefix)?)));
    }
    if self.eat(Token::Tilde)? {
      return Ok(PatKind::Inverse(Box::new(self.parse_pat_bp(BP::Prefix)?)));
    }
    if self.check(Token::OpenParen) {
      let mut tuple = false;
      let mut pats = self.parse_delimited(PAREN_COMMA, |self_| {
        let expr = self_.parse_pat()?;
        if self_.check(Token::Comma) {
          tuple = true;
        }
        Ok(expr)
      })?;
      if pats.len() == 1 && !tuple {
        return Ok(PatKind::Paren(Box::new(pats.pop().unwrap())));
      }
      return Ok(PatKind::Tuple(pats));
    }
    if self.check(Token::OpenBrace) {
      return Ok(PatKind::Object(self.parse_delimited(BRACE_COMMA, |self_| {
        let span = self_.start_span();
        let key = self_.parse_key()?;
        let (pat, ty) = if self_.eat(Token::Colon)? {
          if self_.eat(Token::Colon)? {
            (None, Some(self_.parse_type()?))
          } else {
            (Some(self_.parse_pat()?), None)
          }
        } else if self_.eat(Token::ColonColon)? {
          (None, Some(self_.parse_type()?))
        } else {
          (None, None)
        };
        let span = self_.end_span(span);
        let mut pat =
          pat.unwrap_or_else(|| Pat { span: key.span, kind: PatKind::Path(key.into(), None) });
        if let Some(ty) = ty {
          pat = Pat { span, kind: PatKind::Annotation(Box::new(pat), Box::new(ty)) };
        }
        Ok((key, pat))
      })?));
    }
    self.unexpected()
  }

  fn parse_pat_postfix(
    &mut self,
    lhs: Pat<'core>,
    bp: BP,
  ) -> Parse<'core, Result<PatKind<'core>, Pat<'core>>> {
    if bp.permits(BP::Annotation) && self.eat(Token::Colon)? {
      let ty = self.parse_type()?;
      return Ok(Ok(PatKind::Annotation(Box::new(lhs), Box::new(ty))));
    }
    Ok(Err(lhs))
  }

  fn parse_type(&mut self) -> Parse<'core, Ty<'core>> {
    let span = self.start_span();
    let kind = self._parse_type(span)?;
    let span = self.end_span(span);
    Ok(Ty { span, kind })
  }

  fn _parse_type(&mut self, span: usize) -> Parse<'core, TyKind<'core>> {
    if self.eat(Token::Hole)? {
      return Ok(TyKind::Hole);
    }
    if self.eat(Token::Fn)? {
      let path = self.parse_path()?;
      return Ok(TyKind::Fn(path));
    }
    if self.check(Token::OpenParen) {
      let mut tuple = false;
      let mut types = self.parse_delimited(PAREN_COMMA, |self_| {
        let expr = self_.parse_type()?;
        if self_.check(Token::Comma) {
          tuple = true;
        }
        Ok(expr)
      })?;
      if types.len() == 1 && !tuple {
        return Ok(TyKind::Paren(Box::new(types.pop().unwrap())));
      }
      return Ok(TyKind::Tuple(types));
    }
    if self.check(Token::OpenBrace) {
      return Ok(TyKind::Object(self.parse_delimited(BRACE_COMMA, |self_| {
        let key = self_.parse_key()?;
        self_.expect(Token::Colon)?;
        let value = self_.parse_type()?;
        Ok((key, value))
      })?));
    }
    if self.eat(Token::And)? {
      return Ok(TyKind::Ref(Box::new(self.parse_type()?)));
    }
    if self.eat(Token::AndAnd)? {
      let inner = self.parse_type()?;
      let span = self.end_span(span + 1);
      return Ok(TyKind::Ref(Box::new(Ty { span, kind: TyKind::Ref(Box::new(inner)) })));
    }
    if self.eat(Token::Tilde)? {
      return Ok(TyKind::Inverse(Box::new(self.parse_type()?)));
    }
    if self.check(Token::ColonColon) || self.check(Token::Ident) {
      let path = self.parse_path()?;
      return Ok(TyKind::Path(path));
    }
    self.unexpected()
  }

  fn parse_impl(&mut self) -> Parse<'core, Impl<'core>> {
    let span = self.start_span();
    let kind = self._parse_impl()?;
    let span = self.end_span(span);
    Ok(Impl { span, kind })
  }

  fn _parse_impl(&mut self) -> Result<ImplKind<'core>, Diag<'core>> {
    if self.eat(Token::Hole)? {
      return Ok(ImplKind::Hole);
    }
    if self.check(Token::ColonColon) || self.check(Token::Ident) {
      return Ok(ImplKind::Path(self.parse_path()?));
    }
    if self.eat(Token::Fn)? {
      return Ok(ImplKind::Fn(self.parse_path()?));
    }
    self.unexpected()
  }

  fn parse_trait(&mut self) -> Parse<'core, Trait<'core>> {
    let span = self.start_span();
    let kind = self._parse_trait()?;
    let span = self.end_span(span);
    Ok(Trait { span, kind })
  }

  fn _parse_trait(&mut self) -> Parse<'core, TraitKind<'core>> {
    if self.check(Token::ColonColon) || self.check(Token::Ident) {
      return Ok(TraitKind::Path(self.parse_path()?));
    }
    if self.eat(Token::Fn)? {
      let receiver = self.parse_type()?;
      let params = self.parse_delimited(PAREN_COMMA, Self::parse_type)?;
      let ret = self.eat(Token::ThinArrow)?.then(|| self.parse_type()).transpose()?;
      return Ok(TraitKind::Fn(receiver, params, ret));
    }
    self.unexpected()
  }

  pub(crate) fn parse_stmt(&mut self) -> Parse<'core, Stmt<'core>> {
    let span = self.start_span();
    let kind = if self.eat(Token::Let)? {
      if self.eat(Token::Fn)? {
        let flex = self.parse_flex()?;
        let name = self.parse_ident()?;
        let params = self.parse_delimited(PAREN_COMMA, Self::parse_pat)?;
        let ret = self.eat(Token::ThinArrow)?.then(|| self.parse_type()).transpose()?;
        let body = self.parse_block()?;
        StmtKind::LetFn(LetFnStmt { flex, name, params, ret, body })
      } else {
        let bind = self.parse_pat()?;
        let init = self.eat(Token::Eq)?.then(|| self.parse_expr()).transpose()?;
        let else_block = self.eat(Token::Else)?.then(|| self.parse_block()).transpose()?;
        self.eat(Token::Semi)?;
        StmtKind::Let(LetStmt { bind, init, else_block })
      }
    } else if self.eat(Token::Semi)? {
      StmtKind::Empty
    } else if let Some(item) = self.maybe_parse_item()? {
      StmtKind::Item(item)
    } else if self.check(Token::If)
      || self.check(Token::Match)
      || self.check(Token::Loop)
      || self.check(Token::While)
      || self.check(Token::For)
    {
      let Some(expr) = self.maybe_parse_expr_prefix()? else {
        return self.unexpected();
      };
      let semi = self.eat(Token::Semi)?;
      StmtKind::Expr(expr, semi)
    } else {
      let expr = self.parse_expr()?;
      let semi = self.eat(Token::Semi)?;
      StmtKind::Expr(expr, semi)
    };
    let span = self.end_span(span);
    Ok(Stmt { span, kind })
  }

  fn parse_path(&mut self) -> Parse<'core, Path<'core>> {
    let span = self.start_span();
    let absolute = self.eat(Token::ColonColon)?;
    let segments = self.parse_delimited(PATH, Self::parse_ident)?;
    let generics = self.check(Token::OpenBracket).then(|| self.parse_generic_args()).transpose()?;
    let span = self.end_span(span);
    Ok(Path { span, absolute, segments, generics })
  }

  fn start_span(&self) -> usize {
    self.state.lexer.span().start
  }

  fn end_span(&self, span: usize) -> Span {
    Span { file: self.file, start: span, end: self.state.last_token_end }
  }

  fn span(&self) -> Span {
    let span = self.state.lexer.span();
    Span { file: self.file, start: span.start, end: span.end }
  }
}

#[allow(clippy::absolute_paths)]
type Delimiters = vine_util::parser::Delimiters<Token>;

const PAREN_COMMA: Delimiters = Delimiters {
  open: Some(Token::OpenParen),
  close: Some(Token::CloseParen),
  separator: Some(Token::Comma),
};

const BRACE: Delimiters =
  Delimiters { open: Some(Token::OpenBrace), close: Some(Token::CloseBrace), separator: None };

const BRACE_COMMA: Delimiters = Delimiters {
  open: Some(Token::OpenBrace),
  close: Some(Token::CloseBrace),
  separator: Some(Token::Comma),
};

const BRACKET_COMMA: Delimiters = Delimiters {
  open: Some(Token::OpenBracket),
  close: Some(Token::CloseBracket),
  separator: Some(Token::Comma),
};

const PATH: Delimiters = Delimiters { open: None, close: None, separator: Some(Token::ColonColon) };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Associativity {
  Left,
  Right,
}

/// Binding power.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum BP {
  Min,
  ControlFlow,
  Assignment,
  LogicalImplies,
  LogicalOr,
  LogicalAnd,
  Is,
  Comparison,
  Range,
  BitOr,
  BitXor,
  BitAnd,
  BitShift,
  Additive,
  Multiplicative,
  Exponential,
  Annotation,
  Prefix,
  Max,
}

impl BP {
  const fn inc(self) -> Self {
    if self as u8 == BP::Max as u8 {
      self
    } else {
      unsafe { transmute::<u8, BP>(self as u8 + 1) }
    }
  }

  fn permits(self, other: Self) -> bool {
    other >= self
  }
}

#[rustfmt::skip]
const BINARY_OP_TABLE: &[(BP, Associativity, Token, BinaryOp)] = &[
  (BP::BitOr,          Associativity::Left, Token::Or,       BinaryOp::BitOr),
  (BP::BitXor,         Associativity::Left, Token::Caret,    BinaryOp::BitXor),
  (BP::BitAnd,         Associativity::Left, Token::And,      BinaryOp::BitAnd),
  (BP::BitShift,       Associativity::Left, Token::Shl,      BinaryOp::Shl),
  (BP::BitShift,       Associativity::Left, Token::Shr,      BinaryOp::Shr),
  (BP::Additive,       Associativity::Left, Token::Plus,     BinaryOp::Add),
  (BP::Additive,       Associativity::Left, Token::Minus,    BinaryOp::Sub),
  (BP::Additive,       Associativity::Left, Token::PlusPlus, BinaryOp::Concat),
  (BP::Multiplicative, Associativity::Left, Token::Star,     BinaryOp::Mul),
  (BP::Multiplicative, Associativity::Left, Token::Slash,    BinaryOp::Div),
  (BP::Multiplicative, Associativity::Left, Token::Percent,  BinaryOp::Rem),
  (BP::Exponential,    Associativity::Right, Token::StarStar, BinaryOp::Pow),
];

#[rustfmt::skip]
const COMPARISON_OP_TABLE: &[(Token, ComparisonOp)] = &[
  (Token::EqEq, ComparisonOp::Eq),
  (Token::Ne,   ComparisonOp::Ne),
  (Token::Lt,   ComparisonOp::Lt),
  (Token::Gt,   ComparisonOp::Gt),
  (Token::Le,   ComparisonOp::Le),
  (Token::Ge,   ComparisonOp::Ge),
];
