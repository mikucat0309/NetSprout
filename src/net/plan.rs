use std::collections::{HashMap, HashSet};

/// An existing network interface.
#[derive(Debug, Clone)]
pub struct Link {
    pub ifindex: u32,
    pub name: String,
    pub mac: Option<String>,
    /// Backed by a real device; only these are renamed.
    pub physical: bool,
    pub up: bool,
}

/// Rename a physical interface with this MAC to `name`.
#[derive(Debug, Clone)]
pub struct Want {
    pub mac: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    pub ifindex: u32,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Default)]
pub struct Plan {
    /// To be applied in order.
    pub renames: Vec<Rename>,
    /// Requests that were skipped, and why.
    pub warnings: Vec<String>,
}

const TEMP_PREFIX: &str = "nsrename";

/// Interface names as they will be after the renames planned so far.
struct State<'a> {
    links: &'a [Link],
    current: Vec<String>,
    taken: HashMap<String, usize>,
}

impl State<'_> {
    fn rename(&mut self, idx: usize, to: String) -> Rename {
        self.taken.remove(&self.current[idx]);
        self.taken.insert(to.clone(), idx);
        Rename {
            ifindex: self.links[idx].ifindex,
            from: std::mem::replace(&mut self.current[idx], to.clone()),
            to,
        }
    }
}

/// Work out the renames needed to give each wanted interface its name.
///
/// Requests that cannot be satisfied (no or ambiguous MAC match, interface up,
/// target name held by an interface that is not itself being renamed) are
/// skipped with a warning; the rest are still applied. Swaps and chains are
/// resolved by parking an interface under a temporary name.
pub fn plan(links: &[Link], wants: &[Want]) -> Plan {
    let mut out = Plan::default();
    let mut warn = |msg: String| out.warnings.push(msg);

    // (index into `links`, target name)
    let mut pending: Vec<(usize, &str)> = Vec::new();
    let mut claimed_names = HashSet::new();
    let mut claimed_links = HashSet::new();
    for want in wants {
        if !claimed_names.insert(want.name.as_str()) {
            warn(format!(
                "{}: name already requested for another interface",
                want.name
            ));
            continue;
        }
        let candidates: Vec<usize> = (0..links.len())
            .filter(|&i| links[i].physical && links[i].mac.as_deref() == Some(want.mac.as_str()))
            .collect();
        let idx = match candidates[..] {
            [idx] => idx,
            [] => {
                warn(format!("{}: no interface with MAC {}", want.name, want.mac));
                continue;
            }
            _ => {
                warn(format!(
                    "{}: multiple interfaces with MAC {}",
                    want.name, want.mac
                ));
                continue;
            }
        };
        let link = &links[idx];
        if !claimed_links.insert(idx) {
            warn(format!(
                "{}: {} already has another name requested",
                want.name, link.name
            ));
            continue;
        }
        if link.name == want.name {
            continue;
        }
        if link.up {
            warn(format!("{}: {} is up, not renaming", want.name, link.name));
            continue;
        }
        pending.push((idx, &want.name));
    }

    // A target held by an interface that stays put can never be freed. Dropping
    // such a rename means its interface stays put too, so repeat until stable.
    loop {
        let moving: HashSet<usize> = pending.iter().map(|&(idx, _)| idx).collect();
        let before = pending.len();
        pending.retain(|&(idx, to)| match links.iter().position(|l| l.name == to) {
            Some(holder) if !moving.contains(&holder) => {
                warn(format!(
                    "{to}: name is taken by {}, not renaming {}",
                    links[holder].name, links[idx].name
                ));
                false
            }
            _ => true,
        });
        if pending.len() == before {
            break;
        }
    }

    let mut state = State {
        links,
        current: links.iter().map(|l| l.name.clone()).collect(),
        taken: links
            .iter()
            .enumerate()
            .map(|(i, l)| (l.name.clone(), i))
            .collect(),
    };
    let mut temp_counter = 0;

    while !pending.is_empty() {
        let free = pending
            .iter()
            .position(|(_, to)| !state.taken.contains_key(*to));
        if let Some(pos) = free {
            let (idx, to) = pending.remove(pos);
            out.renames.push(state.rename(idx, to.to_string()));
        } else {
            // Every target is held by another pending interface, i.e. a cycle.
            // Parking one of them frees a name the others are waiting for. Any
            // name not taken right now is also not a pending target.
            let (idx, _) = pending[0];
            let temp = loop {
                let candidate = format!("{TEMP_PREFIX}{temp_counter}");
                temp_counter += 1;
                if !state.taken.contains_key(&candidate) {
                    break candidate;
                }
            };
            out.renames.push(state.rename(idx, temp));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(ifindex: u32, name: &str, mac: &str) -> Link {
        Link {
            ifindex,
            name: name.to_string(),
            mac: Some(mac.to_string()),
            physical: true,
            up: false,
        }
    }

    fn want(mac: &str, name: &str) -> Want {
        Want {
            mac: mac.to_string(),
            name: name.to_string(),
        }
    }

    /// Apply the plan to `links` and return the final ifindex -> name map.
    fn simulate(links: &[Link], plan: &Plan) -> HashMap<u32, String> {
        let mut names: HashMap<u32, String> =
            links.iter().map(|l| (l.ifindex, l.name.clone())).collect();
        for r in &plan.renames {
            assert_eq!(names[&r.ifindex], r.from, "stale `from` in {r:?}");
            assert!(
                !names.values().any(|n| *n == r.to),
                "{r:?} targets a name still in use"
            );
            names.insert(r.ifindex, r.to.clone());
        }
        names
    }

    #[test]
    fn simple_rename() {
        let links = [link(2, "ens3", "aa")];
        let p = plan(&links, &[want("aa", "lan0")]);
        assert!(p.warnings.is_empty());
        assert_eq!(
            p.renames,
            [Rename {
                ifindex: 2,
                from: "ens3".into(),
                to: "lan0".into()
            }]
        );
    }

    #[test]
    fn already_named() {
        let links = [link(2, "lan0", "aa")];
        let p = plan(&links, &[want("aa", "lan0")]);
        assert!(p.renames.is_empty());
        assert!(p.warnings.is_empty());
    }

    #[test]
    fn swap() {
        let links = [link(2, "eth0", "aa"), link(3, "eth1", "bb")];
        let p = plan(&links, &[want("aa", "eth1"), want("bb", "eth0")]);
        assert!(p.warnings.is_empty());
        let names = simulate(&links, &p);
        assert_eq!(names[&2], "eth1");
        assert_eq!(names[&3], "eth0");
        assert_eq!(p.renames.len(), 3);
    }

    #[test]
    fn chain_needs_no_temp() {
        // eth0 -> eth1 -> eth2 (free)
        let links = [link(2, "eth0", "aa"), link(3, "eth1", "bb")];
        let p = plan(&links, &[want("aa", "eth1"), want("bb", "eth2")]);
        let names = simulate(&links, &p);
        assert_eq!(names[&2], "eth1");
        assert_eq!(names[&3], "eth2");
        assert_eq!(p.renames.len(), 2);
    }

    #[test]
    fn three_way_rotation() {
        let links = [link(2, "a", "aa"), link(3, "b", "bb"), link(4, "c", "cc")];
        let p = plan(&links, &[want("aa", "b"), want("bb", "c"), want("cc", "a")]);
        let names = simulate(&links, &p);
        assert_eq!((&*names[&2], &*names[&3], &*names[&4]), ("b", "c", "a"));
    }

    #[test]
    fn temp_name_avoids_existing() {
        let links = [
            link(2, "eth0", "aa"),
            link(3, "eth1", "bb"),
            link(4, "nsrename0", "cc"),
        ];
        let p = plan(&links, &[want("aa", "eth1"), want("bb", "eth0")]);
        let names = simulate(&links, &p);
        assert_eq!(names[&2], "eth1");
        assert_eq!(names[&3], "eth0");
        assert_eq!(names[&4], "nsrename0");
        assert!(p.renames.iter().any(|r| r.to == "nsrename1"));
    }

    #[test]
    fn name_held_by_unmanaged_interface() {
        let mut bond = link(5, "lan0", "zz");
        bond.physical = false;
        let links = [link(2, "ens3", "aa"), bond];
        let p = plan(&links, &[want("aa", "lan0")]);
        assert!(p.renames.is_empty());
        assert_eq!(p.warnings.len(), 1);
    }

    #[test]
    fn blocked_rename_cascades() {
        // eth1 is up so it stays; eth0 then cannot take its name either.
        let mut up = link(3, "eth1", "bb");
        up.up = true;
        let links = [link(2, "eth0", "aa"), up];
        let p = plan(&links, &[want("aa", "eth1"), want("bb", "eth2")]);
        assert!(p.renames.is_empty());
        assert_eq!(p.warnings.len(), 2);
    }

    #[test]
    fn skips_unknown_ambiguous_and_duplicate() {
        let links = [
            link(2, "eth0", "aa"),
            link(3, "eth1", "dup"),
            link(4, "eth2", "dup"),
            link(5, "eth3", "cc"),
        ];
        let p = plan(
            &links,
            &[
                want("missing", "x0"),
                want("dup", "x1"),
                want("aa", "x2"),
                want("cc", "x2"),
                want("aa", "x3"),
            ],
        );
        assert_eq!(
            p.renames,
            [Rename {
                ifindex: 2,
                from: "eth0".into(),
                to: "x2".into()
            }]
        );
        assert_eq!(p.warnings.len(), 4);
    }

    #[test]
    fn ignores_non_physical_with_same_mac() {
        // A VLAN inherits its parent's MAC.
        let mut vlan = link(9, "eth0.10", "aa");
        vlan.physical = false;
        let links = [link(2, "eth0", "aa"), vlan];
        let p = plan(&links, &[want("aa", "lan0")]);
        assert!(p.warnings.is_empty());
        assert_eq!(p.renames.len(), 1);
        assert_eq!(p.renames[0].ifindex, 2);
    }
}
