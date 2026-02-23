import NucleusDB.PaymentChannels.Cuts

namespace NucleusDB
namespace PaymentChannels

/-!
# Blockchain.PaymentChannels.Payments

We model a payment request as a wealth update on the vertex set:
- debit sender `i` by `a`
- credit receiver `j` by `a`

Feasibility is defined as membership of the updated wealth distribution in `WG`.
-/

universe u

namespace Payments

variable {V : Type u} [DecidableEq V]

def pay (w : V → Cap) (i j : V) (a : Cap) : V → Cap :=
  fun v =>
    if i = j then
      w v
    else if v = i then
      w i - a
    else if v = j then
      w j + a
    else
      w v

def PaymentFeasible (G : ChannelGraph V) (w : V → Cap) (i j : V) (a : Cap) : Prop :=
  a ≤ w i ∧ (pay w i j a) ∈ Wealth.WG G

end Payments

end PaymentChannels
end NucleusDB
