# Credits

PercyScarlett's runtime (`runtime/victory`) is built on the work of others. Thank you all.

- **VIctory** by Marvisak and its contributors: the Xbox Series runtime, RDNA2 shader translator and D3D12 layer the runtime is based on. https://github.com/Marvisak/VIctory
- **victory-fork** by fxceless: the fork used as the base, with its speed work, levels on demand and many fixes for TheBig6 😉. https://github.com/fxceless/victory-fork
- **ScarlettPort** by zigzagboi1: the upscale dispatch takeover and stretch pass that fixed the black world on the Series S profile. https://github.com/zigzagboi1/scarlettport
- **XCL** by xcl-dev: research on the console FSR 4 container and the Series S render target formats. https://github.com/xcl-dev/xcl
- Everyone who sent PRs, test reports and issues to these projects and to PercyScarlett.

Our changes on top: the Series S overlay plane blend and full plane description, the game watchdog hold, DirectStorage failure logging, pipeline worker control, texture memory and page-view diagnostics, the serialized pipeline dump, and the prologue script and guide.
