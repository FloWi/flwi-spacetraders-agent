with construction_materials as (select cs.*
                                     , (materials ->> 'required')::int  as required
                                     , (materials ->> 'fulfilled')::int as fulfilled
                                     , materials ->> 'tradeSymbol'      as construction_material_trade_good_symbol
                                from construction_sites cs
                                   , jsonb_array_elements(entry -> 'materials') materials)
select *
from construction_materials;

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
   , market_details as (select m.waypoint_symbol
                             , m.created_at
                             , trade_goods ->> 'type'                 as type
                             , trade_goods ->> 'supply'               as supply
                             , trade_goods ->> 'symbol'               as symbol
                             , trade_goods ->> 'activity'             as activity
                             , (trade_goods ->> 'sellPrice')::int     as sell_price
                             , (trade_goods ->> 'tradeVolume')::int   as trade_volume
                             , (trade_goods ->> 'purchasePrice')::int as purchase_price
                        from markets m
                           , jsonb_array_elements(entry -> 'tradeGoods') as trade_goods)
select m.waypoint_symbol
     , m.created_at
     , m.type
     , m.supply
     , m.symbol
     , m.activity
     , m.sell_price
     , m.trade_volume
     , m.purchase_price
     , supply.supply_level_value
     , activity.activity_level_value
from market_details m
         join activity_values activity
              on m.activity = activity.activity_level
         join supply_levels supply
              on m.supply = supply.supply_level
where type = 'EXPORT'
  and symbol in ('FAB_MATS', 'ADVANCED_CIRCUITRY')
order by symbol
       , created_at
