//! Procedural macros for the [optative](https://crates.io/crates/optative) reconciler
//! library.
//!
//! - [`macro@lifecycle_trace`] — wraps a `Lifecycle` impl's `enter` /
//!   `reconcile_self` / `exit` with `tracing` events.
//! - [`macro@Ephemeral`] — generates a `Drop` impl that reconciles managed sets to
//!   empty so their `exit` hooks run on drop.
//!
//! # Requirements at the call site
//!
//! The code these macros generate refers to crates by name. Any crate that uses
//! `#[lifecycle_trace]` must have **`serde_json`** and **`tracing`** in scope (as
//! standard for derive macros, they are not re-exported by this crate).

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, ImplItem, ItemImpl, parse_macro_input};

/// Wraps an `impl Lifecycle for T` block so each lifecycle transition emits a
/// `tracing` event.
///
/// Injects `wrap_enter`, `wrap_reconcile`, and `wrap_exit` overrides that call
/// the user's `enter` / `reconcile_self` / `exit` and emit `tracing::info!` on
/// success (`"entering"` / `"exiting"`) or `tracing::error!` on failure, with
/// `key`, `display_name`, `metadata`, and `error` fields.
///
/// The generated code calls `serde_json::to_string` and the `tracing` macros, so
/// both crates must be available at the call site.
#[proc_macro_attribute]
pub fn lifecycle_trace(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemImpl);

    let is_lifecycle_impl = input
        .trait_
        .as_ref()
        .and_then(|(_, path, _)| path.segments.last())
        .map(|seg| seg.ident == "Lifecycle")
        .unwrap_or(false);

    if !is_lifecycle_impl {
        return syn::Error::new_spanned(
            &input,
            "#[lifecycle_trace] can only be applied to `impl Lifecycle for T` blocks",
        )
        .to_compile_error()
        .into();
    }

    let entering: ImplItem = syn::parse_quote! {
        fn wrap_enter(self, ctx: &mut Self::Context, output: &mut Self::Output) -> Result<Self::State, Self::Error> {
            let _lc = self.lifecycle_context();
            let _key = self.key();
            let _meta = serde_json::to_string(&_lc.metadata).unwrap_or_default();
            let result = self.enter(ctx, output);
            match &result {
                Ok(_) => tracing::info!(key = ?_key, display_name = %_lc.display_name, metadata = %_meta, "entering"),
                Err(e) => tracing::error!(key = ?_key, display_name = %_lc.display_name, metadata = %_meta, error = %e, "entering failed"),
            }
            result
        }
    };

    let reconciling: ImplItem = syn::parse_quote! {
        fn wrap_reconcile(self, state: &mut Self::State, ctx: &mut Self::Context, output: &mut Self::Output) -> Result<(), Self::Error> {
            let _lc = self.lifecycle_context();
            let _key = self.key();
            let _meta = serde_json::to_string(&_lc.metadata).unwrap_or_default();
            let result = self.reconcile_self(state, ctx, output);
            if let Err(e) = &result {
                tracing::error!(key = ?_key, display_name = %_lc.display_name, metadata = %_meta, error = %e, "reconciling failed");
            }
            result
        }
    };

    let exiting: ImplItem = syn::parse_quote! {
        fn wrap_exit(state: Self::State, ctx: &mut Self::Context, output: &mut Self::Output) -> Result<(), Self::Error> {
            let _lc = Self::lifecycle_state_context(&state);
            let _meta = serde_json::to_string(&_lc.metadata).unwrap_or_default();
            let result = Self::exit(state, ctx, output);
            match &result {
                Ok(_) => tracing::info!(display_name = %_lc.display_name, metadata = %_meta, "exiting"),
                Err(e) => tracing::error!(display_name = %_lc.display_name, metadata = %_meta, error = %e, "exiting failed"),
            }
            result
        }
    };

    input.items.push(entering);
    input.items.push(reconciling);
    input.items.push(exiting);

    quote! { #input }.into()
}

/// Generates a `Drop` impl that reconciles `#[reconciler]`-annotated fields to an
/// empty desired set, running their managed items' `exit` hooks on drop.
///
/// Each field annotated `#[reconciler(output = <field_name>)]` gets a drop-time
/// call of `self.<field>.reconcile(Vec::new(), &mut Default::default(), &mut
/// self.<output_field>)`. The reconciler's `Context` type must implement
/// `Default`.
#[proc_macro_derive(Ephemeral, attributes(reconciler))]
pub fn derive_ephemeral(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => panic!("Ephemeral only supports named fields"),
        },
        _ => panic!("Ephemeral only supports structs"),
    };

    let mut calls = vec![];
    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        for attr in &field.attrs {
            if !attr.path().is_ident("reconciler") {
                continue;
            }
            let mut output_ident: Option<syn::Ident> = None;
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("output")
                    && let syn::Expr::Path(p) = meta.value()?.parse::<syn::Expr>()?
                {
                    output_ident = p.path.get_ident().cloned();
                }
                Ok(())
            })
            .unwrap();
            let output = output_ident.expect("reconciler attribute requires output = <field_name>");
            calls.push(quote! {
                { let mut __ctx = ::core::default::Default::default(); self.#field_name.reconcile(::std::vec::Vec::new(), &mut __ctx, &mut self.#output); }
            });
        }
    }

    quote! {
        impl #impl_generics Drop for #name #ty_generics #where_clause {
            fn drop(&mut self) { #(#calls)* }
        }
    }
    .into()
}
