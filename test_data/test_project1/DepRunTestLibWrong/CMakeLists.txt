
cmake_minimum_required (VERSION 3.8)

project ("DepRunTestCmake")

set(CMAKE_CXX_VISIBILITY_PRESET hidden)
set(CMAKE_VISIBILITY_INLINES_HIDDEN 1)

add_library(DepRunTestLib SHARED "DepRunTestLib.cpp" "DepRunTestLib.h")
include(GenerateExportHeader)
generate_export_header(DepRunTestLib)
target_include_directories(DepRunTestLib PUBLIC . ${CMAKE_CURRENT_BINARY_DIR})
