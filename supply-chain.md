## ADVANCED_CIRCUITRY Supply Chain

```mermaid
%%{init: {"#flowchart": {"htmlLabels": false}} }%%
graph LR
    ELECTRONICS --> ADVANCED_CIRCUITRY
    MICROPROCESSORS --> ADVANCED_CIRCUITRY
    SILICON_CRYSTALS --> ELECTRONICS
    COPPER --> ELECTRONICS
    COPPER_ORE --> COPPER
    SILICON_CRYSTALS --> MICROPROCESSORS
    COPPER --> MICROPROCESSORS
```

## FAB_MATS Supply Chain

```mermaid
%%{init: {"#flowchart": {"htmlLabels": false}} }%%
graph LR
    IRON --> FAB_MATS
    QUARTZ_SAND --> FAB_MATS
    IRON_ORE --> IRON
```

## SHIP_PLATING Supply Chain

```mermaid
%%{init: {"#flowchart": {"htmlLabels": false}} }%%
graph LR
    ALUMINUM --> SHIP_PLATING
    MACHINERY --> SHIP_PLATING
    ALUMINUM_ORE --> ALUMINUM
    IRON --> MACHINERY
    IRON_ORE --> IRON
```

## MICROPROCESSORS Supply Chain

```mermaid
%%{init: {"#flowchart": {"htmlLabels": false}} }%%
graph LR
    SILICON_CRYSTALS --> MICROPROCESSORS
    COPPER --> MICROPROCESSORS
    COPPER_ORE --> COPPER
```

## CLOTHING Supply Chain

```mermaid
%%{init: {"#flowchart": {"htmlLabels": false}} }%%
graph LR
    FABRICS --> CLOTHING
    FERTILIZERS --> FABRICS
    LIQUID_NITROGEN --> FERTILIZERS
```

## Complete Supply Chain

```mermaid
%%{init: {"#flowchart": {"htmlLabels": false}} }%%
graph LR
    ELECTRONICS --> ADVANCED_CIRCUITRY
    MICROPROCESSORS --> ADVANCED_CIRCUITRY
    SILICON_CRYSTALS --> ELECTRONICS
    COPPER --> ELECTRONICS
    COPPER_ORE --> COPPER
    SILICON_CRYSTALS --> MICROPROCESSORS
    COPPER --> MICROPROCESSORS
    IRON --> FAB_MATS
    QUARTZ_SAND --> FAB_MATS
    IRON_ORE --> IRON
    ALUMINUM --> SHIP_PLATING
    MACHINERY --> SHIP_PLATING
    ALUMINUM_ORE --> ALUMINUM
    IRON --> MACHINERY
    FABRICS --> CLOTHING
    FERTILIZERS --> FABRICS
    LIQUID_NITROGEN --> FERTILIZERS
```

ranked supply chain sorted:

```text
#0: IRON_ORE
#0: SILICON_CRYSTALS
#0: LIQUID_NITROGEN
#0: ALUMINUM_ORE
#0: COPPER_ORE
#0: QUARTZ_SAND
#1: COPPER
#1: IRON
#1: ALUMINUM
#1: FERTILIZERS
#2: MICROPROCESSORS
#2: FAB_MATS
#2: ELECTRONICS
#2: FABRICS
#2: MACHINERY
#3: CLOTHING
#3: ADVANCED_CIRCUITRY
#3: SHIP_PLATING
```
