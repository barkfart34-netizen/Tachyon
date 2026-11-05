# Third-party notices

## wgpu-hal

`third_party/wgpu-hal` is [wgpu-hal](https://github.com/gfx-rs/wgpu) 30.0.1 by the gfx-rs
developers, with one upstream fix applied
([gfx-rs/wgpu#10124](https://github.com/gfx-rs/wgpu/pull/10124)). It is licensed under MIT or
Apache-2.0; both licenses are in that folder (`LICENSE.MIT`, `LICENSE.APACHE`).

## Pixelify Sans

`crates/horizon-theme/fonts/PixelifySans-*.ttf` is [Pixelify Sans](https://github.com/eifetx/Pixelify-Sans)
by The Pixelify Sans Project Authors, licensed under the SIL Open Font License 1.1
(`crates/horizon-theme/fonts/OFL-PixelifySans.txt`).

## Pixelarticons

`crates/horizon-theme/icons/pixel/*.svg` are icons from
[Pixelarticons](https://github.com/halfmage/pixelarticons) by Gerrit Halfmann, under the MIT
license (`crates/horizon-theme/icons/pixel/LICENSE-pixelarticons.txt`). `star.svg` is Horizon's own, drawn on
the same grid.

## Zalith Launcher 2

`crates/horizon-client/src/controls/default_layout.json` is the default control layout of
[Zalith Launcher 2](https://github.com/ZalithLauncher/ZalithLauncher2) by MovTery and
contributors (its `default_layout.json`, with its own name and author changed), licensed under
the GNU General Public License 3.0. Horizon reads and writes its layout format so that layouts
move between the two; none of Zalith's code is in Horizon.

The Android app (`android/build.sh`) carries the OpenJDK 25 runtime for Android that Zalith
Launcher 2 ships, built from OpenJDK, which is licensed under the GNU General Public License 2.0
with the Classpath Exception. It runs only the preparation's exporters, as a separate program.

## horizon-android touch controls

Tachyon's Bedrock-style touch formats, value contracts, movement/button engine and HUD in
`crates/horizon-client-render/src/touch.rs` and `touch/{layout,custom,hud}.rs` follow
[Vasysik/horizon-android](https://github.com/Vasysik/horizon-android), branch `android`, commit
`7301f6fdd7f470b11317276e7a11b4dc615c137a`. The source files followed are
`crates/horizon-client-render/src/touch.rs` (controls, `TouchControls::draw`, sprite, text and circle helpers),
`crates/horizon-client-render/src/touch/layout.rs` (layouts and hit-test geometry),
`crates/horizon-client-render/src/touch/custom.rs` (stored buttons and `fit_placement`), and
`crates/horizon-client-input/src/gamepad.rs` (only `java_move_vector`, ported to the same path).
The fork's code is licensed under MIT:

```
MIT License

Copyright (c) 2026 Ludovic Marie and the Horizon contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

This notice covers code, not Mojang's artwork. No touch textures or other assets are included.

## AMD FidelityFX SDK (FSR 3.1 frame generation)

`crates/horizon-client-render/src/gfx/frame_gen/optical_flow.wgsl` is a port to WGSL of the
optical flow shaders of the [AMD FidelityFX SDK](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK)
1.1.4 (`sdk/include/FidelityFX/gpu/opticalflow`, at commit
[c6efa6bf7f20](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/tree/c6efa6bf7f2027b3ec94f28578bb5965eabb9e55)),
and `optical_flow.rs` follows the SDK's `sdk/src/components/opticalflow/ffx_opticalflow.cpp`
(its passes, their order and their dispatch sizes). Copyright (C) 2024 Advanced Micro Devices,
Inc., under the MIT license, whose text heads both files. AMD and FidelityFX are AMD's
trademarks; Horizon is not affiliated with AMD.

## Mod loader logos

The launcher shows the logo of the mod loader an instance plays with.

- `launcher/assets/logos/quilt.png` is the Quilt mini icon (`brand/128png/quilt_mini_icon_transparent.png`)
  from [QuiltMC/art](https://github.com/QuiltMC/art), by the Quilt Project, dedicated to the public
  domain under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/).
- `launcher/assets/logos/neoforge.png` is NeoForged's icon (`art/neoforged_icon_16x16.png`) from
  [neoforged/.github](https://github.com/neoforged/.github/tree/main/art), © the NeoForged team,
  created by Ridanisaurus, under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). It is
  unmodified (the launcher scales it up by whole pixels).
- Fabric's and Forge's icons are not shipped: neither publishes a license for its logo. The launcher
  fetches them from their own sites when an instance needs them
  ([fabric-loader's icon](https://github.com/FabricMC/fabric-loader/blob/master/src/main/resources/assets/fabricloader/icon.png),
  [files.minecraftforge.net's](https://files.minecraftforge.net/)).

Each logo belongs to its project. Showing it says which loader an instance runs; it does not mean
the project endorses Horizon.

## Sign in with Microsoft

The launcher's sign-in buttons follow Microsoft's published branding guidelines for apps that sign in
with Microsoft accounts
([Sign in with Microsoft branding](https://learn.microsoft.com/en-us/entra/identity-platform/howto-add-branding-in-apps)).
The four-square Microsoft symbol in `launcher/src/microsoft.rs` reproduces the squares, positions and
colours of Microsoft's official `ms-symbollockup_mssymbol_19.svg` from that page, unmodified. Microsoft
and its logo are trademarks of the Microsoft group of companies; Horizon is not endorsed by Microsoft.

## Create

Tachyon's machines follow the code of [Create](https://github.com/Creators-of-Create/Create)
(tag `mc1.21.1-6.0.10`): its crushing wheels (`crates/horizon-client/src/game/tachyon_engine/crusher.rs`,
from `CrushingWheelBlock`, `CrushingWheelControllerBlock` and `CrushingWheelControllerBlockEntity`),
its water wheels (`water_wheel.rs`, from `WaterWheelBlockEntity`, `LargeWaterWheelBlockEntity`,
`LargeWaterWheelBlock` and `WaterWheelRenderer`), its encased cogwheels (`kinetic.rs`,
`tachyon_engine.rs` and `crates/horizon-client/src/tachyon/create_port.rs`, from `AllBlocks`,
`EncasedCogwheelBlock`, `ICogWheel`, `RotationPropagator`, `KineticBlockEntityVisual`,
`EncasedCogRenderer`, `EncasedCogVisual`, `AllPartialModels` and `BuilderTransformers`),
its recipe format (`create_recipes.rs`),
its gauges (`kinetic.rs`, `tachyon_engine.rs` and `crates/horizon-client/src/tachyon/create_port.rs`,
from `GaugeBlock`, `DirectionalAxisKineticBlock`, `GaugeGenerator`, `DirectionalAxisBlockStateGen`,
`GaugeBlockEntity`, `SpeedGaugeBlockEntity`, `StressGaugeBlockEntity`, `IRotate`, `GaugeRenderer`,
`ShaftRenderer` and `AllBlocks`), its goggles (`tachyon_engine.rs` and `tachyon/create_port.rs`,
from `AllItems.GOGGLES` and `GogglesItem`),
its rotation speed controllers (`speed_controller.rs`, from `SpeedControllerBlock`,
`SpeedControllerBlockEntity`, `SpeedControllerRenderer` and `RotationPropagator`),
and its machines' numbers in `kinetic.rs`, `millstone.rs`, `basin.rs`, `fan.rs`, `belt.rs` and
`tachyon_engine.rs` (capacities, the water wheel's flow, the millstone's work, the mixer's time,
the fan's reach, the funnel's stack). Create's code is under the MIT license:

```
Copyright (c) The Create Team / The Creators of Create

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

Create's assets are not under that license (All Rights Reserved) and are not in this repository or
its releases: the game fetches Create from Modrinth on the player's device and reads them there.
