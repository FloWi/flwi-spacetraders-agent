with trade_good_sells as (select (entry -> 'TicketCompleted' ->> 'total')::int                                   as sell_total
                               , entry -> 'TicketCompleted' ->> 'fleet_id'                                       as fleet_id
                               , entry -> 'TicketCompleted' -> 'finance_ticket' ->> 'ticket_id'                  as ticket_id
                               , (entry -> 'TicketCompleted' ->> 'actual_units') ::int                           as actual_units
                               , entry -> 'TicketCompleted' -> 'finance_ticket'                                  as finance_ticket
                               , entry -> 'TicketCompleted' -> 'finance_ticket' -> 'details' -> 'SellTradeGoods' as sell_trade_goods_entry
                               , entry -> 'TicketCompleted' ->> 'actual_price_per_unit'                          as sell_actual_price_per_unit
                               , jsonb_object_keys(entry -> 'TicketCompleted' -> 'finance_ticket' -> 'details')  AS ticket_type
                               , entry
                               , created_at
                          from ledger_entries
                          where entry -> 'TicketCompleted' -> 'finance_ticket' -> 'details' -> 'SellTradeGoods' is not null
                          order by created_at desc)
   , trade_good_purchases as (
    (select (entry -> 'TicketCompleted' ->> 'total')::int                                       as purchase_total
          , entry -> 'TicketCompleted' ->> 'fleet_id'                                           as fleet_id
          , entry -> 'TicketCompleted' -> 'finance_ticket' ->> 'ticket_id'                      as ticket_id
          , (entry -> 'TicketCompleted' ->> 'actual_units') ::int                               as actual_units
          , entry -> 'TicketCompleted' -> 'finance_ticket'                                      as finance_ticket
          , entry -> 'TicketCompleted' -> 'finance_ticket' -> 'details' -> 'PurchaseTradeGoods' as purchase_trade_goods_entry
          , entry -> 'TicketCompleted' ->> 'actual_price_per_unit'                              as purchase_actual_price_per_unit
          , jsonb_object_keys(entry -> 'TicketCompleted' -> 'finance_ticket' -> 'details')      AS ticket_type
          , entry
          , created_at
     from ledger_entries
     where entry -> 'TicketCompleted' -> 'finance_ticket' -> 'details' -> 'PurchaseTradeGoods' is not null
     order by created_at desc))
   , trade_good_sells_details as (select sell_total
                                       , fleet_id
                                       , ticket_id
                                       , actual_units
                                       , sell_actual_price_per_unit
                                       , ticket_type
                                       , sell_trade_goods_entry ->> 'quantity'                                          as quantity
                                       , sell_trade_goods_entry ->> 'trade_good'                                        as trade_good
                                       , sell_trade_goods_entry ->> 'waypoint_symbol'                                   as waypoint_symbol
                                       , (sell_trade_goods_entry ->> 'expected_price_per_unit')::int                    as expected_price_per_unit
                                       , (sell_trade_goods_entry ->> 'expected_total_sell_price')::int                  as expected_total_sell_price
                                       , sell_trade_goods_entry ->> 'maybe_matching_purchase_ticket'                    as maybe_matching_purchase_ticket
                                       , entry -> 'TicketCompleted' ->> 'actual_price_per_unit'                         as sell_actual_price_per_unit
                                       , jsonb_object_keys(entry -> 'TicketCompleted' -> 'finance_ticket' -> 'details') AS ticket_type
                                       , entry
                                       , created_at
                                  from trade_good_sells)
   , trade_good_purchase_details as (select purchase_total
                                          , fleet_id
                                          , ticket_id
                                          , actual_units
                                          , purchase_actual_price_per_unit
                                          , ticket_type
                                          , purchase_trade_goods_entry ->> 'quantity'                                      as quantity
                                          , purchase_trade_goods_entry ->> 'trade_good'                                    as trade_good
                                          , purchase_trade_goods_entry ->> 'waypoint_symbol'                               as waypoint_symbol
                                          , (purchase_trade_goods_entry ->> 'expected_price_per_unit')::int                as expected_price_per_unit
                                          , (purchase_trade_goods_entry ->> 'expected_total_purchase_price')::int          as expected_total_purchase_price
                                          , entry -> 'TicketCompleted' ->> 'actual_price_per_unit'                         as purchase_actual_price_per_unit
                                          , jsonb_object_keys(entry -> 'TicketCompleted' -> 'finance_ticket' -> 'details') AS ticket_type
                                          , entry
                                          , created_at
                                     from trade_good_purchases)
   , aggregated_trades as (select sells.trade_good
                                , purchases.waypoint_symbol
                                , sells.waypoint_symbol
                                , count(*)                                            num_trades
                                , sum(sells.actual_units)                             sum_units
                                , sum(sells.sell_total - purchases.purchase_total) as sum_profit
                                , avg(sells.sell_total - purchases.purchase_total) as avg_profit

                           from trade_good_sells_details sells
                                    join trade_good_purchase_details purchases
                                         on maybe_matching_purchase_ticket = purchases.ticket_id
                           group by sells.trade_good, purchases.waypoint_symbol, sells.waypoint_symbol)
select *
from aggregated_trades
;
