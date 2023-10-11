# fp_app

<!-- [![Dependency status](https://deps.rs/repo/github/Barugon/fp_app/status.svg)](https://deps.rs/repo/github/Barugon/fp_app) -->

This will eventually be a VFR flight planner that uses free FAA assets. Currently, you can open and view [charts](https://www.faa.gov/air_traffic/flight_info/aeronav/digital_products/vfr/) (zipped GEO-TIFF). You can also open the [NASR 28 day subscription](https://www.faa.gov/air_traffic/flight_info/aeronav/aero_data/NASR_Subscription/) zip file in order to search for airports.

If running on a device like PinePhone or Librem 5 then use the command line parameter `--no-deco` in order to run without window decorations. Compiling with `--features=phosh` will also exclude window decorations.
