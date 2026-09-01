# Changelog

## 0.1.0 (2026-09-01)


### Features

* an environment held by leftover processes can be seen and stopped ([ee8f004](https://github.com/Project-Colony/Raven/commit/ee8f0042f020461218d154c670f04228f3787d2c))
* doctor names who actually gets a double-clicked .exe ([f3d4e04](https://github.com/Project-Colony/Raven/commit/f3d4e0453ac27c31014468c1929083957b6c0582))
* environments, base deployment, and one command that runs a program ([a747487](https://github.com/Project-Colony/Raven/commit/a7474872c5c1abeef0b0de1afa95f37950c6b97f))
* hide the base's side-by-side store, and a real installer works ([c721240](https://github.com/Project-Colony/Raven/commit/c721240894e908e15450250c3274b18d5207b641))
* locate Raven's files under Colony/Raven, with names that cannot escape ([45e6acf](https://github.com/Project-Colony/Raven/commit/45e6acf3c104ac5a01c371e2768374e777b6da18))
* make ./program.exe run, and stop panicking on a closed pipe ([5f13680](https://github.com/Project-Colony/Raven/commit/5f1368087709f7cf110907eb571703a76345e98c))
* mask the base's Fonts directory - process spawn drops 227 to 135 ms ([7d6ecff](https://github.com/Project-Colony/Raven/commit/7d6ecffaa680b25d79b6dee12ac6e92c31b5b91a))
* mount an immutable base under a writable overlay, without root ([3b7d165](https://github.com/Project-Colony/Raven/commit/3b7d16554a61fc595c709a61ac430e8af32ad11e))
* project a real Windows registry into the prefix, selectively ([a8cc81a](https://github.com/Project-Colony/Raven/commit/a8cc81a756fefb9b9f194cddb37f8a7ccb04a3cf))
* rename a layer's paths to a reference Windows's spelling ([2ba3bc3](https://github.com/Project-Colony/Raven/commit/2ba3bc3276a5bcadb26097db0ebbb04f59c17fba))
* stack read-only layers so Wine's files win over Microsoft's ([594d692](https://github.com/Project-Colony/Raven/commit/594d6920be8574db91e0fec39c55ba689d694ee9))


### Bug Fixes

* doctor no longer hides hand-disabled .exe registrations ([63daaf4](https://github.com/Project-Colony/Raven/commit/63daaf45342933c035c7f4d02bf808f18f070c82))
* gate the missing-layer test on userns like its siblings ([4dbefac](https://github.com/Project-Colony/Raven/commit/4dbefac4689ef6135e3d40abd884153959b5e3aa))
* holders were blind to upper paths containing ',' ':' or '\' ([8c24424](https://github.com/Project-Colony/Raven/commit/8c244245af8dcf0f8beabb02b81079ef021df43a))
* Raven cannot share the .exe registration with Wine ([f2578bc](https://github.com/Project-Colony/Raven/commit/f2578bcfd93eaacaeb0aab37c5bc4d9a6cf5e607))
* the availability check now reads all three userns gates ([64d87bc](https://github.com/Project-Colony/Raven/commit/64d87bcaa48082279f4e3932ccf51fb9515727ad))
* wait for the registry to reach disk before the import returns ([32a1e3c](https://github.com/Project-Colony/Raven/commit/32a1e3c883f8de39da21e7f7f56ee95ef4254d6e))


### Performance Improvements

* adopt a measured release profile ([402eaac](https://github.com/Project-Colony/Raven/commit/402eaac8322d8f659d911daad096998c5b19c3d2))
