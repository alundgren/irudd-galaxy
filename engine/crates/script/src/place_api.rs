//! The place commands on ctx. Kept separate from screen and other host bindings.

use galaxy_api::Command;
use mlua::{Lua, Table, Value};

use super::Env;

pub(super) fn bind<'scope, 'env: 'scope>(
    _lua: &Lua,
    scope: &'scope mlua::Scope<'scope, 'env>,
    env: &'env Env<'env>,
    ctx: &Table,
) -> mlua::Result<()> {
    ctx.raw_set("move", scope.create_function(
        |lua, (entity, place, spot, state): (String, String, Option<String>, Option<String>)| {
            env.write(lua, Command::Move {
                entity: env.entity(lua, &entity)?, place: env.entity(lua, &place)?,
                spot: spot.map(|s| env.entity(lua, &s)).transpose()?, state,
            })
        },
    )?)?;
    ctx.raw_set(
        "walk",
        scope.create_function(
            |lua, (entity, spot, speed, state): (String, String, f64, Option<String>)| {
                env.write(
                    lua,
                    Command::Walk {
                        entity: env.entity(lua, &entity)?,
                        spot: env.entity(lua, &spot)?,
                        speed,
                        state,
                    },
                )
            },
        )?,
    )?;
    ctx.raw_set(
        "advance_clock",
        scope.create_function(|lua, (entity, amount): (String, Value)| {
            let amount = crate::vm::whole_number(&amount).ok_or_else(|| {
                env.fail(
                    lua,
                    "bad_command",
                    "advance_clock takes a nonnegative whole amount",
                )
            })?;
            env.write(
                lua,
                Command::AdvanceClock {
                    entity: env.entity(lua, &entity)?,
                    amount,
                },
            )
        })?,
    )?;
    Ok(())
}
