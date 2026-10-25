# Motion

<p align="center"><img src="images/ori/working.svg" width="120" alt="Ori typing on a laptop"></p>

How everything in Alloy moves. The rule, shared with Orbit: **motion says what happened.**
Things you start get a moment of delight (the forge), things you use a hundred times a day
stay quick, and loops only run for a state or the scene.

## Tokens

Defined once in `src/styles.css`.

| Token | Value | For |
| --- | --- | --- |
| `--t-tap` | 90 ms | press feedback |
| `--t-quick` | 160 ms | hover, colour, small state changes |
| `--t-move` | 260 ms | things that travel: popovers, pills, underlines |
| `--t-show` | 420 ms | entrances |
| `--ease-out` | `cubic-bezier(.22, 1, .36, 1)` | arriving |
| `--ease-in` | `cubic-bezier(.55, 0, .75, .1)` | leaving (shorter than arriving) |
| `--spring` | `cubic-bezier(.34, 1.4, .64, 1)` | small overshoot for things you caused |
| `--stagger` | 35 ms | per item in a list, capped at 12 items |

**Settings → Look and feel → Motion:** Full (everything), Calm (slower loops), Off
(every change instant). The system's reduced-motion setting is respected too.
**Pausing:** all loops pause while a game is running or Alloy is in the background, so the
launcher never takes GPU time from Minecraft.

## Every element

| Element | Appears | Hover | Press | Idle | Leaves |
| --- | --- | --- | --- | --- | --- |
| **Play block** | rises with its page | lifts 1 px; after 0.9 s heats: bevel to molten, glow grows, then molten metal flows over the face (WebGL, CSS fallback), sparks fly | sinks by its bevel; when hot, a spark burst | a light glint crosses the gold every 7 s; running: green dot breathes | cools back to gold over 0.65 s |
| **Small Play** (lists) | with its card | steel turns gold | sinks | still | |
| **Buttons** | with their page | lift 1 px, icon nudges up | 97% | still | |
| **Gold buttons** | with their page | brighter, soft gold shadow | 97% | still | |
| **Icon buttons** (rail, title bar) | with the window | background fades in, icon grows 10% | 90% | badges pop in with a spring | |
| **Rail** | | tooltip slides in from the left | | the active marker and background slide to the current page | |
| **Logo** | halves slide in from both sides and lock; molten light flashes through the seam (once per session) | halves press together, seam glows | squashes to 94% | still | |
| **Cards** (instances, results, actions) | rise and fade, staggered | lift 2 px, border brightens, shadow | 99% | still | |
| **Instance icons** | with their card | tilt in 3D toward you | | still | morph into the instance page header |
| **Segmented controls** | with their row | text brightens | 95% | the pill slides to the choice | |
| **Tabs** | with the header | text brightens | | the underline slides | |
| **Switches** | with their row | | knob springs across | | |
| **Text fields** | with their page | border lightens | | | |
| | | **Focus:** border goes gold, soft ring fades in | | | **Error:** one 3 px shake |
| **Page title** | slides up into place | | | | slides up and out |
| **Pages** | fade and rise 10 px | | | | fade and lift 6 px, faster |
| **Dialogs** | backdrop fades, panel springs up from 96% | close button turns 90° | | | drops and fades, faster |
| **Toasts** | spring up from below, Ori poses for the kind | | | a thin line counts down | slide out to the right |
| **Menus** | fade from their corner | items highlight | | | |
| **Progress** | | | | a light shimmer moves along the bar | |
| **Install button** | | | | spinner while installing | the check pops in |
| **Empty states** | rise with Ori | | | Ori's own loop | |

## The forge (background)

| Layer | Moves |
| --- | --- |
| The scene | drifts in scale over 60 s; parallax follows the mouse a few pixels |
| Molten metal (seam, pours, channels) | light-gold highlight breathes, out of phase |
| Brazier fire | flickers |
| Smoke | drifts and swells over 18 s |
| Embers | rise from the crucible, the seam and the brazier, and fade |

Home shows the scene sharp; other pages show a pre-blurred copy, darker.
