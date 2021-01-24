# CMakeList.txt : CMake project for DepRunTest, include source and define
# project specific logic here.
#
cmake_minimum_required (VERSION 3.8)

# Add source to this project's executable.
add_executable (DepRunTest "DepRunTest.cpp")

target_link_libraries(DepRunTest PRIVATE DepRunTestLib)

# TODO: Add tests and install targets if needed.
