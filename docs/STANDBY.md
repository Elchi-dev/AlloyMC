# Warm standby (experimental, opt-in)

<p align="center"><img src="images/ori/sleeping.svg" width="120" alt="Ori asleep"></p>

Big modpacks take minutes to start. Warm standby makes Play near-instant by doing the
start-up ahead of time. Everything here is a separate switch in Settings.

## Layers

| Layer | What it does | Cost |
| --- | --- | --- |
| Disk pre-warm | Reads the pack's jars and assets at low priority so the OS has them cached | none; cache is reclaimable |
| Java startup cache | Java 25's ahead-of-time cache, created after the first launch, reused after | a few hundred MB on disk |
| ModernFix suggestion | Offers ModernFix for big packs | none |
| **Standby** | Starts the pack hidden, lets it load, then freezes it; Play unfreezes and shows it | see below |

## Standby settings

| Setting | Options |
| --- | --- |
| Memory | **Keep in RAM** (instant, holds the RAM) · **Page out** (Windows pagefile, Linux memory reclaim; with zram it lands in compressed RAM) · **Hybrid** (keep N GB ready, page out the rest) |
| CPU | Frozen, or lowest priority |
| When to arm | When Alloy starts · after you quit the game · on hover over Play |
| Which instances | Per instance, plus a maximum number at once |
| When to release | Low RAM · another game starts · on battery · after N hours |

## Limits, stated plainly

- Some video memory stays reserved while a pack is in standby.
- The Minecraft token lasts 24 hours; Alloy refreshes a standby before it expires.
- Switching accounts releases standbys.
- A small helper mod keeps the game window hidden until you press Play.
