--!strict
-- The tool is bought only with the work done and the price in hand; the ledger counts
-- the edits made to the world from outside it.

return {
	on = {
		["purse.buy"] = function(event, ctx)
			local wallet = ctx.get("buyer", "wallet")
			local goods = ctx.get("tool", "goods")
			if not wallet or not goods or goods.owned then
				return
			end
			if wallet.coins >= goods.price and wallet.work >= goods.work then
				ctx.patch("buyer", "wallet", { coins = wallet.coins - goods.price })
				ctx.patch("tool", "goods", { owned = true })
				ctx.record("bought", {})
			else
				ctx.record("refused", {})
			end
		end,
		-- Debug only: coins paid in, never past 1000 (a rule an edit would skip).
		["purse.grant"] = function(event, ctx)
			local wallet = ctx.get("buyer", "wallet")
			if wallet then
				ctx.patch("buyer", "wallet", { coins = math.min(1000, wallet.coins + event.payload.coins) })
			end
		end,
		["engine.edited"] = function(event, ctx)
			local ledger = ctx.get("book", "ledger")
			if ledger then
				ctx.patch("book", "ledger", { edits = ledger.edits + 1, last = event.payload.source })
			end
		end,
	},
}
