cmake_minimum_required(VERSION 3.20)
# Firmware de banco de F1 (TEST-04/TEST-05). El componente mole es el padre.
set(EXTRA_COMPONENT_DIRS "../..")
include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(mole_bench)
