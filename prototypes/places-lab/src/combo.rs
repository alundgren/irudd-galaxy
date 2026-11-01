//! The four choices and the variant picked for each. Keys `1`-`4` cycle them (Shift goes
//! back); `--combo cut,staged,bubble,still` picks them from the command line.

use render::Renderer;

use crate::life::{self, Life};
use crate::places::Places;
use crate::room::{self, Room};
use crate::talk::{self, Talk};
use crate::travel::{self, Travel};
use crate::world::World;

pub const CHOICES: [&str; 4] = ["travel", "room", "talk", "life"];

pub struct Combo {
    pub travels: Vec<Box<dyn Travel>>,
    pub rooms: Vec<Box<dyn Room>>,
    pub talks: Vec<Box<dyn Talk>>,
    pub lives: Vec<Box<dyn Life>>,
    /// The picked variant of each choice, in [`CHOICES`] order.
    pub pick: [usize; 4],
}

impl Combo {
    pub fn new() -> Combo {
        Combo {
            travels: travel::all(),
            rooms: room::all(),
            talks: talk::all(),
            lives: life::all(),
            pick: [0; 4],
        }
    }

    /// Lets every variant upload what it draws with.
    pub fn setup(&mut self, r: &mut Renderer, world: &World, model: &Places) {
        for v in &mut self.travels {
            v.setup(r, world, model);
        }
        for v in &mut self.rooms {
            v.setup(r, world, model);
        }
        for v in &mut self.talks {
            v.setup(r, world, model);
        }
        for v in &mut self.lives {
            v.setup(r, world, model);
        }
    }

    pub fn room_mut(&mut self) -> &mut dyn Room {
        self.rooms[self.pick[1]].as_mut()
    }

    pub fn travel(&self) -> &dyn Travel {
        self.travels[self.pick[0]].as_ref()
    }
    pub fn travel_mut(&mut self) -> &mut dyn Travel {
        self.travels[self.pick[0]].as_mut()
    }
    pub fn room(&self) -> &dyn Room {
        self.rooms[self.pick[1]].as_ref()
    }
    pub fn talk(&self) -> &dyn Talk {
        self.talks[self.pick[2]].as_ref()
    }
    pub fn talk_mut(&mut self) -> &mut dyn Talk {
        self.talks[self.pick[2]].as_mut()
    }
    pub fn life(&self) -> &dyn Life {
        self.lives[self.pick[3]].as_ref()
    }
    pub fn life_mut(&mut self) -> &mut dyn Life {
        self.lives[self.pick[3]].as_mut()
    }

    /// The variants' names for one choice.
    pub fn names(&self, choice: usize) -> Vec<&'static str> {
        match choice {
            0 => self.travels.iter().map(|v| v.name()).collect(),
            1 => self.rooms.iter().map(|v| v.name()).collect(),
            2 => self.talks.iter().map(|v| v.name()).collect(),
            _ => self.lives.iter().map(|v| v.name()).collect(),
        }
    }

    /// The next (or previous) variant of a choice.
    pub fn cycle(&mut self, choice: usize, back: bool) {
        if choice == 0 {
            // A move under way lands before the way of moving changes.
            self.travel_mut().finish();
        }
        let n = self.names(choice).len();
        let p = &mut self.pick[choice];
        *p = if back { (*p + n - 1) % n } else { (*p + 1) % n };
    }

    /// Picks variants by name, in choice order: "cut,staged,bubble,still". Missing
    /// entries keep their pick; `-` skips one.
    pub fn set(&mut self, spec: &str) -> Result<(), String> {
        for (choice, name) in spec.split(',').map(str::trim).enumerate() {
            if choice >= 4 {
                return Err(format!("--combo has four parts at most: {spec:?}"));
            }
            if name.is_empty() || name == "-" {
                continue;
            }
            let names = self.names(choice);
            match names.iter().position(|n| *n == name) {
                Some(i) => self.pick[choice] = i,
                None => {
                    return Err(format!(
                        "no {} variant {name:?}; there are: {}",
                        CHOICES[choice],
                        names.join(", ")
                    ));
                }
            }
        }
        Ok(())
    }

    /// "cut,staged,bubble,still": the combination in use, as `--combo` takes it.
    pub fn spec(&self) -> String {
        (0..4)
            .map(|c| self.names(c)[self.pick[c]])
            .collect::<Vec<_>>()
            .join(",")
    }

    /// For the corner label: "1 cut · 2 staged · 3 bubble · 4 still".
    pub fn label(&self) -> String {
        (0..4)
            .map(|c| format!("{} {}", c + 1, self.names(c)[self.pick[c]]))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_combo_round_trips_through_its_spec() {
        let mut c = Combo::new();
        let spec = c.spec();
        c.cycle(0, false);
        c.cycle(3, true);
        c.set(&spec).unwrap();
        assert_eq!(c.spec(), spec);
        assert!(c.set("teleport").is_err());
    }
}
