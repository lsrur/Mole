# SPDX-License-Identifier: MIT
# Componente ESP-IDF de Mole v2 (FW-02). Las fuentes se agregan por tarea.
idf_component_register(
    SRCS
        "src/mole_codec.cpp"
        "src/mole_core.cpp"
        "src/mole_task.cpp"
        "src/mole_port_esp.cpp"
        "src/mole_transport_uart.cpp"
        "src/mole_transport_cdc.cpp"
    INCLUDE_DIRS "include"
    PRIV_INCLUDE_DIRS "src"
    PRIV_REQUIRES driver esp_timer esp_system esp_app_format esp_hw_support
)

target_compile_options(${COMPONENT_LIB} PRIVATE -Wall -Wextra)
