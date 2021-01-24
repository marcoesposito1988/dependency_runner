cmake_minimum_required (VERSION 3.8)

project ("DepRunTestCmake")

option(SAME_OUTPUT "Output .exe and .dll into same library, so that the project runs out of the box")
if (SAME_OUTPUT)
    set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY ${PROJECT_BINARY_DIR}/lib)
    set(CMAKE_LIBRARY_OUTPUT_DIRECTORY ${PROJECT_BINARY_DIR}/lib)
    set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${PROJECT_BINARY_DIR}/bin)
endif()

add_subdirectory ("DepRunTestLib")
add_subdirectory ("DepRunTest")