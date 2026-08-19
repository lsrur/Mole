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

enable_testing()
add_test(
  NAME codec_test
  COMMAND codec_test ${CMAKE_CURRENT_SOURCE_DIR}/../../../protocol/codec_vectors.json
)
