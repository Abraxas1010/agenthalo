namespace HeytingLean
namespace NucleusDB
namespace Identity

/-- Identity safety posture selected by the operator. -/
inductive IdentitySecurityTier where
  | maxSafe
  | lessSafe
  | lowSecurity
  deriving DecidableEq, Repr

/-- Device identity material captured by the runtime. -/
structure DeviceIdentity where
  enabled : Bool
  entropyBits : Nat
  browserFingerprintPresent : Bool
  pufFingerprintPresent : Bool
  deriving DecidableEq, Repr

/-- Network identity sharing surface. -/
structure NetworkIdentity where
  shareLocalIp : Bool
  sharePublicIp : Bool
  shareMac : Bool
  localIpHashPresent : Bool
  publicIpHashPresent : Bool
  macCount : Nat
  deriving DecidableEq, Repr

/-- Full logical identity state.
This is a formal join over two runtime persistence units:
`profile.json` (name/lock/revision) and `identity_config.json`
(mode/tier/device/network). The Lean model is atomic; runtime writes are
performed on separate files and must be treated as a refinement step. -/
structure IdentityState where
  profileName : Option String
  profileNameLocked : Bool
  profileNameRevision : Nat
  anonymousMode : Bool
  securityTier : Option IdentitySecurityTier
  device : Option DeviceIdentity
  network : Option NetworkIdentity
  deriving DecidableEq, Repr

def NetworkIdentity.empty : NetworkIdentity where
  shareLocalIp := false
  sharePublicIp := false
  shareMac := false
  localIpHashPresent := false
  publicIpHashPresent := false
  macCount := 0

/-- Backend predicate used for dashboard `network_configured`. -/
def networkConfigured (n : NetworkIdentity) : Bool :=
  n.shareLocalIp
    || n.sharePublicIp
    || n.shareMac
    || n.localIpHashPresent
    || n.publicIpHashPresent
    || (n.macCount > 0)

theorem networkConfigured_empty_false :
    networkConfigured NetworkIdentity.empty = false := by
  rfl

theorem networkConfigured_of_shareLocalIp
    (n : NetworkIdentity) (h : n.shareLocalIp = true) :
    networkConfigured n = true := by
  simp [networkConfigured, h]

theorem networkConfigured_monotone_addMac
    (n : NetworkIdentity) :
    networkConfigured { n with macCount := n.macCount + 1 } = true := by
  have hPos : 0 < ({ n with macCount := n.macCount + 1 }).macCount := by
    simp
  simp [networkConfigured, hPos]

end Identity
end NucleusDB
end HeytingLean
