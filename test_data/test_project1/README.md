##Test data

- DepRunTest
  
  Working project with executable DepRunTest and DepRunTestLib
  - cmake-build-debug: built with CLion, won't run out of the box (requires user to specify path)
  - build: built with Visual Studio, won't run out of the box (requires user to specify path)
  - build-same-output: built with Visual Studio to output exe and dll into same folder (will run out of the box)
  - build-vcxproj-user: built with Visual Studio, contains .vcxproj.user so that it will run out of the box
    
- DepRunTestLibWrong
  
  Alternative implementation of DepRunTestLib, with function of same name but wrong arguments
  

## Tests

- wldd
  - [ ] build-same-output, everything must be found
  - [ ] build, dll must be missing  
- deprun 
  - [ ] build
    - [ ] no extra settings: fail
    - [ ] manual override: must succeed
    - [ ] dwp file: must succeed
  - [ ] cmake-build-debug: same as build  
  - [ ] build-vcxproj-user: must succeed when pointing to .vcxproj  
  - [ ] build-same-output: must succeed 