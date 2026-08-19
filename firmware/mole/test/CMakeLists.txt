cmake_minimum_required(VERSION 3.20)
project(mole_codec_test CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_library(mole_codec STATIC ../src/mole_codec.cpp)
target_include_directories(mole_codec PUBLIC ../include)
target_compile_options(mole_codec PRIVATE -Wall -Wextra -Werror)

add_executable(codec_test main.cpp)
target_link_libraries(codec_test PRIVATE mole_codec)
target_include_directories(codec_test PRIVATE vendor)

# Tests del core (F1). Compila el core con un límite de símbolos chico para
# poder probar el overflow; usa los mocks de puerto de host (R-2).
add_executable(core_test
  core_test.cpp
  mole_port_host.cpp
  ../src/mole_core.cpp
)
target_include_directories(core_test PRIVATE ../include ../src)
target_compile_definitions(core_test PRIVATE MOLE_MAX_SYMBOLS=4)
target_compile_options(core_test PRIVATE -Wall -Wextra -Werror)
target_link_libraries(core_test PRIVATE mole_codec)
find_package(Threads REQUIRED)
target_link_libraries(core_test PRIVATE Threads::Threads)

enable_testing()
add_test(
  NAME codec_test
  COMMAND codec_test ${CMAKE_CURRENT_SOURCE_DIR}/../../../protocol/codec_vectors.json
)
add_test(NAME core_test COMMAND core_test)
