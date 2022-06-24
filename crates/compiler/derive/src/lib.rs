//! Auto-derivers of builtin ability methods.

use std::iter::once;
use std::sync::{Arc, Mutex};

use roc_can::pattern::Pattern;
use roc_can::{def::Def, module::ExposedByModule};
use roc_collections::MutMap;
use roc_derive_key::DeriveKey;
use roc_module::symbol::{IdentIds, ModuleId, Symbol};
use roc_region::all::Loc;
use roc_types::subs::{Content, Descriptor, Mark, OptVariable, Rank, Subs, Variable};

mod encoding;

pub(crate) const DERIVED_MODULE: ModuleId = ModuleId::DERIVED;

pub fn synth_var(subs: &mut Subs, content: Content) -> Variable {
    let descriptor = Descriptor {
        content,
        // NOTE: this is incorrect, but that is irrelevant - derivers may only be called during
        // monomorphization (or later), at which point we do not care about variable
        // generalization. Hence ranks should not matter.
        rank: Rank::toplevel(),
        mark: Mark::NONE,
        copy: OptVariable::NONE,
    };
    subs.fresh(descriptor)
}

/// Map of [`DeriveKey`]s to their derived symbols.
#[derive(Debug, Default)]
pub struct DerivedModule {
    map: MutMap<DeriveKey, (Symbol, Def)>,
    subs: Subs,
    derived_ident_ids: IdentIds,

    /// Has someone stolen subs/ident ids from us?
    #[cfg(debug_assertions)]
    stolen: bool,
}

pub struct StolenFromDerived {
    pub subs: Subs,
    pub ident_ids: IdentIds,
}

fn build_derived_body(
    derived_subs: &mut Subs,
    derived_ident_ids: &mut IdentIds,
    exposed_by_module: &ExposedByModule,
    derived_symbol: Symbol,
    derive_key: DeriveKey,
) -> Def {
    let (body, var) = match derive_key {
        DeriveKey::ToEncoder(to_encoder_key) => {
            let mut env = encoding::Env {
                subs: derived_subs,
                exposed_types: exposed_by_module,
                derived_ident_ids,
            };
            encoding::derive_to_encoder(&mut env, to_encoder_key)
        }
        DeriveKey::Decoding => todo!(),
    };

    Def {
        loc_pattern: Loc::at_zero(Pattern::Identifier(derived_symbol)),
        loc_expr: Loc::at_zero(body),
        expr_var: var,
        pattern_vars: once((derived_symbol, var)).collect(),
        annotation: None,
    }
}

impl DerivedModule {
    pub fn get_or_insert(
        &mut self,
        exposed_by_module: &ExposedByModule,
        key: DeriveKey,
    ) -> &(Symbol, Def) {
        #[cfg(debug_assertions)]
        {
            debug_assert!(!self.stolen, "attempting to add to stolen symbols!");
        }

        // TODO: can we get rid of the clone?
        let entry = self.map.entry(key.clone());

        entry.or_insert_with(|| {
            let ident_id = if cfg!(debug_assertions) || cfg!(feature = "debug-derived-symbols") {
                let debug_name = key.debug_name();
                debug_assert!(
                    self.derived_ident_ids.get_id(&debug_name).is_none(),
                    "duplicate debug name for different derive key"
                );
                let ident_id = self.derived_ident_ids.get_or_insert(&debug_name);

                // This is expensive, but yields much better symbols when debugging.
                // TODO: hide behind debug_flags?
                DERIVED_MODULE.register_debug_idents(&self.derived_ident_ids);

                ident_id
            } else {
                self.derived_ident_ids.gen_unique()
            };

            let derived_symbol = Symbol::new(DERIVED_MODULE, ident_id);
            let derived_def = build_derived_body(
                &mut self.subs,
                &mut self.derived_ident_ids,
                exposed_by_module,
                derived_symbol,
                key.clone(),
            );

            (derived_symbol, derived_def)
        })
    }

    pub fn iter_all(&self) -> impl Iterator<Item = (&DeriveKey, &(Symbol, Def))> {
        self.map.iter()
    }

    /// Generate a unique symbol. This should only be used when generating code inside the Derived
    /// module; other modules should use [`Self::get_or_insert`] to generate a symbol for a derived
    /// ability member usage.
    pub fn gen_unique(&mut self) -> Symbol {
        let ident_id = self.derived_ident_ids.gen_unique();
        Symbol::new(DERIVED_MODULE, ident_id)
    }

    /// Steal all created derived ident Ids.
    /// After this is called, [`Self::get_or_insert`] may no longer be called.
    pub fn steal(&mut self) -> StolenFromDerived {
        let mut ident_ids = Default::default();
        std::mem::swap(&mut self.derived_ident_ids, &mut ident_ids);
        let mut subs = Default::default();
        std::mem::swap(&mut self.subs, &mut subs);

        #[cfg(debug_assertions)]
        {
            debug_assert!(!self.stolen);
            self.stolen = true;
        }

        StolenFromDerived { subs, ident_ids }
    }

    pub fn return_stolen(&mut self, stolen: StolenFromDerived) {
        #[cfg(debug_assertions)]
        {
            debug_assert!(self.stolen);
            self.stolen = false;
        }

        let StolenFromDerived { subs, ident_ids } = stolen;

        self.subs = subs;
        self.derived_ident_ids = ident_ids;
    }
}

/// Thread-sharable [`DerivedMethods`].
pub type SharedDerivedModule = Arc<Mutex<DerivedModule>>;
