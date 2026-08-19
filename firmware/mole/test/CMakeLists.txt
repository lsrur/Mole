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

# REC-33: un fmt cuya cantidad de {} no coincide con los argumentos NO debe
# compilar. Control positivo primero: el uso correcto SÍ compila (si no,
# el chequeo negativo sería vacuo).
file(WRITE ${CMAKE_CURRENT_BINARY_DIR}/holes_ok.cpp
"#include \"mole.h\"\nvoid f() { MOLE_INFO(\"x={} y={}\", 1, 2); }\n")
file(WRITE ${CMAKE_CURRENT_BINARY_DIR}/holes_fail.cpp
"#include \"mole.h\"\nvoid f() { MOLE_INFO(\"x={} y={}\", 1); }\n")
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)
try_compile(HOLES_BIEN_COMPILA
  ${CMAKE_CURRENT_BINARY_DIR}/holes_check_ok
  SOURCES ${CMAKE_CURRENT_BINARY_DIR}/holes_ok.cpp
  CMAKE_FLAGS "-DINCLUDE_DIRECTORIES=${CMAKE_CURRENT_SOURCE_DIR}/../include;${CMAKE_CURRENT_SOURCE_DIR}/../src"
  CXX_STANDARD 17
)
try_compile(HOLES_MAL_COMPILA
  ${CMAKE_CURRENT_BINARY_DIR}/holes_check
  SOURCES ${CMAKE_CURRENT_BINARY_DIR}/holes_fail.cpp
  CMAKE_FLAGS "-DINCLUDE_DIRECTORIES=${CMAKE_CURRENT_SOURCE_DIR}/../include;${CMAKE_CURRENT_SOURCE_DIR}/../src"
  CXX_STANDARD 17
)
unset(CMAKE_TRY_COMPILE_TARGET_TYPE)
if(NOT HOLES_BIEN_COMPILA)
  message(FATAL_ERROR "el control positivo de REC-33 no compila: chequeo vacuo")
endif()
if(HOLES_MAL_COMPILA)
  message(FATAL_ERROR "REC-33 roto: un log con {} de mas COMPILO y debia fallar")
endif()

enable_testing()
add_test(
  NAME codec_test
  COMMAND codec_test ${CMAKE_CURRENT_SOURCE_DIR}/../../../protocol/codec_vectors.json
)
add_test(NAME core_test COMMAND core_test)
