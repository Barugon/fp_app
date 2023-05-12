# fp_app

[![Dependency status](https://deps.rs/repo/github/Barugon/fp_app/status.svg)](https://deps.rs/repo/github/Barugon/fp_app)

This will eventually be a VFR flight planner that uses free FAA assets. Currently, you can open and view [charts](https://www.faa.gov/air_traffic/flight_info/aeronav/digital_products/vfr/) (zipped GEO-TIFF). You can also open [NASR data files](https://www.faa.gov/air_traffic/flight_info/aeronav/aero_data/NASR_Subscription/) (zipped CSV) in order to search for airports.

If running on a device like PinePhone or Librem 5 then use the command line parameter `--no-deco` in order to run without window decorations.

> **Note**: Pinch-zoom and long press are now working. The software keyboard, however, will not not come up automatically as this appears to be a deficiency in [winit](https://github.com/rust-windowing/winit/issues/1823).
