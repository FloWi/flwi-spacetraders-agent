-- Add migration script here
UPDATE transactions
SET tx_summary = jsonb_set(
        tx_summary,
        '{transaction_action_event}',
        CASE
            WHEN tx_summary -> 'transaction_action_event' ? 'PurchasedTradeGoods' THEN
                jsonb_build_object(
                        'PurchasedTradeGoods', jsonb_build_object(
                        'ticket_details', tx_summary -> 'transaction_action_event' -> 'PurchasedTradeGoods' -> 0,
                        'response', tx_summary -> 'transaction_action_event' -> 'PurchasedTradeGoods' -> 1
                                               )
                )
            WHEN tx_summary -> 'transaction_action_event' ? 'SoldTradeGoods' THEN
                jsonb_build_object(
                        'SoldTradeGoods', jsonb_build_object(
                        'ticket_details', tx_summary -> 'transaction_action_event' -> 'SoldTradeGoods' -> 0,
                        'response', tx_summary -> 'transaction_action_event' -> 'SoldTradeGoods' -> 1
                                          )
                )
            WHEN tx_summary -> 'transaction_action_event' ? 'SuppliedConstructionSite' THEN
                jsonb_build_object(
                        'SuppliedConstructionSite', jsonb_build_object(
                        'ticket_details', tx_summary -> 'transaction_action_event' -> 'SuppliedConstructionSite' -> 0,
                        'response', tx_summary -> 'transaction_action_event' -> 'SuppliedConstructionSite' -> 1
                                                    )
                )
            WHEN tx_summary -> 'transaction_action_event' ? 'ShipPurchased' THEN
                jsonb_build_object(
                        'ShipPurchased', jsonb_build_object(
                        'ticket_details', tx_summary -> 'transaction_action_event' -> 'ShipPurchased' -> 0,
                        'response', tx_summary -> 'transaction_action_event' -> 'ShipPurchased' -> 1
                                         )
                )
            END
                 )
WHERE tx_summary -> 'transaction_action_event' ? 'PurchasedTradeGoods'
   OR tx_summary -> 'transaction_action_event' ? 'SoldTradeGoods'
   OR tx_summary -> 'transaction_action_event' ? 'SuppliedConstructionSite'
   OR tx_summary -> 'transaction_action_event' ? 'ShipPurchased';
