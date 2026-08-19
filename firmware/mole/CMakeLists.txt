# SPDX-License-Identifier: MIT
# Componente ESP-IDF de Mole v2 (FW-02). Las fuentes se agregan por tarea.
idf_component_register(
    SRCS
        "src/mole_codec.cpp"
        "src/mole_core.cpp"
        "src/mole_task.cpp"
        "src/mole_port_esp.cpp"
    INCLUDE_DIRS "include"
    PRIV_INCLUDE_DIRS "src"
    PRIV_REQUIRES esp_timer
)

target_compile_options(${COMPONENT_LIB} PRIVATE -Wall -Wextra)
