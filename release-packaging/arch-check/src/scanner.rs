use crate::model::Report;
use std::{collections::BTreeSet, path::Path};
use syn::{
    UseTree, Visibility,
    visit::{self, Visit},
};

fn flatten_use(tree: &UseTree, prefix: Vec<String>, out: &mut Vec<Vec<String>>) {
    match tree {
        UseTree::Path(p) => {
            let mut pfx = prefix;
            pfx.push(p.ident.to_string());
            flatten_use(&p.tree, pfx, out);
        }
        UseTree::Name(n) => {
            let mut pfx = prefix;
            pfx.push(n.ident.to_string());
            out.push(pfx);
        }
        UseTree::Rename(n) => {
            let mut pfx = prefix;
            pfx.push(n.ident.to_string());
            out.push(pfx);
        }
        UseTree::Glob(_) => out.push(prefix),
        UseTree::Group(g) => {
            for t in &g.items {
                flatten_use(t, prefix.clone(), out);
            }
        }
    }
}

fn token_ident(tokens: proc_macro2::TokenStream, target: &str) -> Option<proc_macro2::Span> {
    for token in tokens {
        match token {
            proc_macro2::TokenTree::Ident(ident) if ident == target => return Some(ident.span()),
            proc_macro2::TokenTree::Group(group) => {
                if let Some(span) = token_ident(group.stream(), target) {
                    return Some(span);
                }
            }
            _ => {}
        }
    }
    None
}

pub(crate) struct Scan<'a> {
    pub(crate) report: &'a mut Report,
    pub(crate) crate_name: &'a str,
    pub(crate) file: &'a Path,
    pub(crate) module: Vec<String>,
    pub(crate) edges: &'a mut BTreeSet<(String, String)>,
    pub(crate) modules: &'a mut BTreeSet<String>,
}
impl Scan<'_> {
    fn location(&self, span: proc_macro2::Span) -> String {
        format!("{}:{}", self.file.display(), span.start().line)
    }
    fn module_id(&self, module: &[String]) -> String {
        format!("{}::{}", self.crate_name, module.join("::"))
    }
    fn path(&mut self, path: &[String], span: proc_macro2::Span) {
        if path.is_empty() {
            return;
        }
        let mut target = self.module.clone();
        let mut n = 0;
        match path[0].as_str() {
            "crate" => {
                target.clear();
                n = 1;
            }
            "self" => n = 1,
            "super" => {
                while path.get(n).is_some_and(|s| s == "super") {
                    target.pop();
                    n += 1;
                }
            }
            _ => {
                target.clear();
            }
        }
        target.extend(path[n..].iter().cloned());
        let from = self.module_id(&self.module);
        // Resolve item imports to their longest known containing module after traversal.
        let to = self.module_id(&target);
        self.edges.insert((from, to));
        let layer = self
            .module
            .iter()
            .find(|s| ["api", "domain", "ports"].contains(&s.as_str()));
        let target_layer = target.iter().find(|s| {
            [
                "api",
                "domain",
                "ports",
                "registry",
                "strategies",
                "adapters",
            ]
            .contains(&s.as_str())
        });
        if let (Some(a), Some(b)) = (layer, target_layer)
            && ((a == "domain"
                && ["api", "registry", "strategies", "adapters"].contains(&b.as_str()))
                || (a == "ports" && ["api", "domain"].contains(&b.as_str())))
        {
            self.report.add(
                "layer",
                self.location(span),
                format!("{a} must not depend on {b}"),
            );
        }
    }
    fn function(&mut self, sig: &syn::Signature, block: &syn::Block) {
        self.report.metrics.functions += 1;
        let mut visitor = Complexity { score: 1 };
        visitor.visit_block(block);
        let key = format!("{}:{}", self.location(sig.ident.span()), sig.ident);
        self.report
            .metrics
            .function_complexity
            .insert(key.clone(), visitor.score);
        self.report.metrics.max_complexity = self.report.metrics.max_complexity.max(visitor.score);
        if visitor.score > 20 {
            self.report.add(
                "complexity",
                self.location(sig.ident.span()),
                format!("{} complexity {} exceeds 20", sig.ident, visitor.score),
            );
        }
        if sig.ident == "new" {
            let count = sig
                .inputs
                .iter()
                .filter(|i| matches!(i, syn::FnArg::Typed(_)))
                .count();
            self.report
                .metrics
                .constructor_dependencies
                .insert(key, count);
            self.report.metrics.max_constructor_dependencies =
                self.report.metrics.max_constructor_dependencies.max(count);
            if count > 4 {
                self.report.add(
                    "constructor-dependencies",
                    self.location(sig.ident.span()),
                    format!("new has {count} parameters, maximum 4"),
                );
            }
        }
    }
}
struct Complexity {
    score: usize,
}
impl<'ast> Visit<'ast> for Complexity {
    fn visit_expr_if(&mut self, i: &'ast syn::ExprIf) {
        self.score += 1;
        visit::visit_expr_if(self, i);
    }
    fn visit_expr_for_loop(&mut self, i: &'ast syn::ExprForLoop) {
        self.score += 1;
        visit::visit_expr_for_loop(self, i);
    }
    fn visit_expr_while(&mut self, i: &'ast syn::ExprWhile) {
        self.score += 1;
        visit::visit_expr_while(self, i);
    }
    fn visit_expr_loop(&mut self, i: &'ast syn::ExprLoop) {
        self.score += 1;
        visit::visit_expr_loop(self, i);
    }
    fn visit_arm(&mut self, i: &'ast syn::Arm) {
        self.score += 1;
        visit::visit_arm(self, i);
    }
    fn visit_expr_binary(&mut self, i: &'ast syn::ExprBinary) {
        if matches!(i.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) {
            self.score += 1;
        }
        visit::visit_expr_binary(self, i);
    }
}
impl<'ast> Visit<'ast> for Scan<'_> {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        for attribute in &i.attrs {
            let remapped = attribute.path().is_ident("path")
                || (attribute.path().is_ident("cfg_attr")
                    && match &attribute.meta {
                        syn::Meta::List(list) => token_ident(list.tokens.clone(), "path").is_some(),
                        _ => false,
                    });
            if remapped {
                self.report.add(
                    "module-path",
                    self.location(i.ident.span()),
                    "remapped module paths are unsupported by the architecture graph",
                );
            }
        }
        let n = i.ident.to_string();
        if ["common", "util", "helpers", "misc"].contains(&n.as_str()) {
            self.report.add(
                "module-name",
                self.location(i.ident.span()),
                format!("module name {n} is forbidden"),
            );
        }
        if let Some((_, items)) = &i.content {
            self.module.push(n);
            self.modules.insert(self.module_id(&self.module));
            for item in items {
                self.visit_item(item);
            }
            self.module.pop();
        }
    }
    fn visit_macro(&mut self, i: &'ast syn::Macro) {
        if i.path.is_ident("include") || token_ident(i.tokens.clone(), "mod").is_some() {
            self.report.add(
                "module-macro",
                self.location(i.path.segments[0].ident.span()),
                "include! and macro-generated modules are unsupported by the architecture graph",
            );
        }
        if self.crate_name != "platform-os"
            && let Some(span) = token_ident(i.tokens.clone(), "unsafe")
        {
            self.report.add(
                "unsafe",
                self.location(span),
                "unsafe token in macro outside platform-os",
            );
        }
        visit::visit_macro(self, i);
    }
    fn visit_item_use(&mut self, i: &'ast syn::ItemUse) {
        let mut paths = Vec::new();
        flatten_use(&i.tree, Vec::new(), &mut paths);
        for path in paths {
            self.path(&path, i.use_token.span);
        }
    }
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if let Some(first) = path.segments.first() {
            let segments = path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>();
            self.path(&segments, first.ident.span());
        }
        visit::visit_path(self, path);
    }
    fn visit_vis_restricted(&mut self, _: &'ast syn::VisRestricted) {
        // The path in pub(in ...) controls visibility without referencing an item.
    }
    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        if self.crate_name == "core-contracts" && matches!(i.vis, Visibility::Public(_)) {
            self.report.add(
                "core-public-function",
                self.location(i.sig.ident.span()),
                "public free function in core-contracts",
            );
        }
        if matches!(i.sig.safety, syn::Safety::Unsafe(_)) && self.crate_name != "platform-os" {
            self.report.add(
                "unsafe",
                self.location(i.sig.ident.span()),
                "unsafe function outside platform-os",
            );
        }
        self.function(&i.sig, &i.block);
        visit::visit_item_fn(self, i);
    }
    fn visit_impl_item_fn(&mut self, i: &'ast syn::ImplItemFn) {
        if matches!(i.sig.safety, syn::Safety::Unsafe(_)) && self.crate_name != "platform-os" {
            self.report.add(
                "unsafe",
                self.location(i.sig.ident.span()),
                "unsafe function outside platform-os",
            );
        }
        self.function(&i.sig, &i.block);
        visit::visit_impl_item_fn(self, i);
    }
    fn visit_trait_item_fn(&mut self, i: &'ast syn::TraitItemFn) {
        if matches!(i.sig.safety, syn::Safety::Unsafe(_)) && self.crate_name != "platform-os" {
            self.report.add(
                "unsafe",
                self.location(i.sig.ident.span()),
                "unsafe trait function outside platform-os",
            );
        }
        if let Some(block) = &i.default {
            self.function(&i.sig, block);
        }
        visit::visit_trait_item_fn(self, i);
    }
    fn visit_foreign_item_fn(&mut self, i: &'ast syn::ForeignItemFn) {
        if matches!(i.sig.safety, syn::Safety::Unsafe(_)) && self.crate_name != "platform-os" {
            self.report.add(
                "unsafe",
                self.location(i.sig.ident.span()),
                "unsafe foreign function outside platform-os",
            );
        }
        visit::visit_foreign_item_fn(self, i);
    }
    fn visit_expr_unsafe(&mut self, i: &'ast syn::ExprUnsafe) {
        if self.crate_name != "platform-os" {
            self.report.add(
                "unsafe",
                self.location(i.unsafe_token.span),
                "unsafe block outside platform-os",
            );
        }
        visit::visit_expr_unsafe(self, i);
    }
    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if let Some(token) = i.unsafety
            && self.crate_name != "platform-os"
        {
            self.report.add(
                "unsafe",
                self.location(token.span),
                "unsafe impl outside platform-os",
            );
        }
        visit::visit_item_impl(self, i);
    }
    fn visit_item_trait(&mut self, i: &'ast syn::ItemTrait) {
        if let Some(token) = i.unsafety
            && self.crate_name != "platform-os"
        {
            self.report.add(
                "unsafe",
                self.location(token.span),
                "unsafe trait outside platform-os",
            );
        }
        visit::visit_item_trait(self, i);
    }
    fn visit_item_foreign_mod(&mut self, i: &'ast syn::ItemForeignMod) {
        if let Some(token) = i.unsafety
            && self.crate_name != "platform-os"
        {
            self.report.add(
                "unsafe",
                self.location(token.span),
                "unsafe extern outside platform-os",
            );
        }
        visit::visit_item_foreign_mod(self, i);
    }
}
