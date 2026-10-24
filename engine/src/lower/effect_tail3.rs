//! Effects of the single-card tail, part 3 (M7c, ENGINE.md 30). Same rules
//! as `effect.rs`: every field named, the rest denied.

use super::Lowerer;
use crate::program::{Amount, Builder, Ext, Op};
use card_ir::{cats, nodes};

/// Lower an effect class of the tail, part 3; `false` if the class is not
/// one of these.
pub(crate) fn lower_effect_tail3(l: &mut Lowerer, b: &mut Builder, e: &cats::Effect) -> bool {
    use cats::Effect as E;
    match e {
        // "Create twice X 1/1 ... tokens" (Pest Infestation: the spell's X).
        E::CreateTokensScaledByX(nodes::CreateTokensScaledByX { token_printing, multiplier }) => {
            if !l.spell_x {
                l.unsupported("CreateTokensScaledByX(no X)");
            }
            let k = l.int("CreateTokensScaledByX.multiplier", *multiplier, 1, 4) as u8;
            let cats::CardPrinting::CardPrinting(tp) = &**token_printing;
            let def = super::printing::lower_printing(l, tp);
            if !def.token {
                l.unsupported("CreateTokensScaledByX.token_printing(not a token)");
            }
            let token = l.token(def);
            b.op(Op::Ext(Ext::CreateTokensN { token, n: Amount::XTimes(k), tapped: false, attacking: false }));
            b.op(Op::Ext(Ext::TokensCreated));
        }
        _ => return false,
    }
    true
}
