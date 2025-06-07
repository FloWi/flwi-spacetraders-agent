with supply_levels as (SELECT *
                       FROM (VALUES ('SCARCE', 0),
                                    ('LIMITED', 1),
                                    ('MODERATE', 2),
                                    ('HIGH', 3),
                                    ('ABUNDANT', 4)) AS mapping(supply_level, supply_level_value))
   , activity_values as (SELECT *
                         FROM (VALUES ('STRONG', 4),
                                      ('GROWING', 3),
                                      ('WEAK', 2),
                                      ('RESTRICTED', 1)) AS mapping(activity_level, activity_level_value))
   , latest_market_entries as (select distinct on (waypoint_symbol) *
                               from markets
                               order by waypoint_symbol, created_at desc)
   , market_details as (select m.waypoint_symbol
                             , m.created_at
                             , trade_goods ->> 'type'                 as type
                             , trade_goods ->> 'supply'               as supply
                             , trade_goods ->> 'symbol'               as symbol
                             , trade_goods ->> 'activity'             as activity
                             , (trade_goods ->> 'sellPrice')::int     as sell_price
                             , (trade_goods ->> 'tradeVolume')::int   as trade_volume
                             , (trade_goods ->> 'purchasePrice')::int as purchase_price
                        from latest_market_entries m
                           , jsonb_array_elements(entry -> 'tradeGoods') as trade_goods)
   , sell_market_entries as (select *
                             from market_details m
                             where type in ('EXCHANGE', 'EXPORT'))
   , purchase_market_entries as (select *
                                 from market_details m
                                 where type in ('EXCHANGE', 'IMPORT'))
select s.symbol                        as trade_good_symbol
     , s.waypoint_symbol               as s_waypoint_symbol
     , s.type                          as s_type
     , s.trade_volume                  as s_trade_volume
     , s.activity                      as s_activity
     , s.supply                        as s_supply_level
     , s.sell_price                    as s_sell_price
     , s.purchase_price                as s_purchase_price
     , p.sell_price - s.purchase_price as profit
     , p.sell_price                    as p_sell_price
     , p.purchase_price                as p_purchase_price
     , p.supply                        as p_supply_level
     , p.activity                      as p_activity
     , p.trade_volume                  as p_trade_volume
     , p.type                          as p_type
     , p.waypoint_symbol               as p_waypoint_symbol
     , *
from sell_market_entries s
         cross join purchase_market_entries p
where p.waypoint_symbol <> s.waypoint_symbol
  and p.symbol = s.symbol
  and p.type <> s.type
order by profit desc


