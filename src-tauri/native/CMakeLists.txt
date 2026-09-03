cmake_minimum_required(VERSION 3.20)

project(tauterm_trdp_native C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_C_EXTENSIONS ON)

option(TAUTERM_TRDP_SANITIZE "Enable AddressSanitizer/UBSan for native TRDP diagnostics" OFF)

set(TCNOPEN_ROOT "${CMAKE_CURRENT_LIST_DIR}/../vendor/tcnopen")
set(TCNOPEN_API "${TCNOPEN_ROOT}/src/api")
set(TCNOPEN_COMMON "${TCNOPEN_ROOT}/src/common")
set(TCNOPEN_VOS_API "${TCNOPEN_ROOT}/src/vos/api")
set(TCNOPEN_VOS_COMMON "${TCNOPEN_ROOT}/src/vos/common")

set(TCNOPEN_CORE_SOURCES
    "${TCNOPEN_COMMON}/trdp_pdcom.c"
    "${TCNOPEN_COMMON}/trdp_mdcom.c"
    "${TCNOPEN_COMMON}/trdp_utils.c"
    "${TCNOPEN_COMMON}/trdp_stats.c"
    "${TCNOPEN_COMMON}/tlp_if.c"
    "${TCNOPEN_COMMON}/tlm_if.c"
    "${TCNOPEN_COMMON}/tlc_if.c"
    "${TCNOPEN_VOS_COMMON}/vos_utils.c"
    "${TCNOPEN_VOS_COMMON}/vos_mem.c"
)

if(WIN32)
    set(TCNOPEN_VOS_PLATFORM "${TCNOPEN_ROOT}/src/vos/windows")
    list(APPEND TCNOPEN_CORE_SOURCES
        "${TCNOPEN_VOS_PLATFORM}/vos_sock.c"
        "${TCNOPEN_VOS_PLATFORM}/vos_thread.c"
        "${TCNOPEN_VOS_PLATFORM}/vos_shared_mem.c"
    )
    set(TCNOPEN_PLATFORM_DEFINITIONS
        WIN64
        MD_SUPPORT=1
        L_ENDIAN
        _CRT_SECURE_NO_WARNINGS
        _WINSOCK_DEPRECATED_NO_WARNINGS
    )
    set(TCNOPEN_PLATFORM_LIBRARIES
        ws2_32
        iphlpapi
        winmm
    )
else()
    find_package(Threads REQUIRED)
    set(TCNOPEN_VOS_PLATFORM "${TCNOPEN_ROOT}/src/vos/posix")
    list(APPEND TCNOPEN_CORE_SOURCES
        "${TCNOPEN_VOS_PLATFORM}/vos_sock.c"
        "${TCNOPEN_VOS_PLATFORM}/vos_thread.c"
        "${TCNOPEN_VOS_PLATFORM}/vos_shared_mem.c"
    )
    set(TCNOPEN_PLATFORM_DEFINITIONS
        POSIX
        MD_SUPPORT=1
        L_ENDIAN
    )
    set(TCNOPEN_PLATFORM_LIBRARIES
        Threads::Threads
        ${CMAKE_DL_LIBS}
    )
    if(APPLE)
        list(APPEND TCNOPEN_PLATFORM_DEFINITIONS
            __USE_BSD
            _DARWIN_C_SOURCE
        )
    else()
        list(APPEND TCNOPEN_PLATFORM_DEFINITIONS
            LINUX
            _GNU_SOURCE
        )
        list(APPEND TCNOPEN_PLATFORM_LIBRARIES rt)
    endif()
endif()

add_library(tauterm_tcnopen STATIC ${TCNOPEN_CORE_SOURCES})
target_include_directories(tauterm_tcnopen
    PUBLIC
        "${TCNOPEN_API}"
        "${TCNOPEN_VOS_API}"
    PRIVATE
        "${TCNOPEN_COMMON}"
        "${TCNOPEN_VOS_PLATFORM}"
)
target_compile_definitions(tauterm_tcnopen PRIVATE ${TCNOPEN_PLATFORM_DEFINITIONS})
set_target_properties(tauterm_tcnopen PROPERTIES POSITION_INDEPENDENT_CODE ON)

if(MSVC)
    target_compile_options(tauterm_tcnopen PRIVATE /W3 /wd4068)
else()
    target_compile_options(tauterm_tcnopen PRIVATE
        -Wall
        -Wno-unknown-pragmas
        -Wno-unused-label
        -Wno-unused-function
    )
endif()

set(BRIDGE_SOURCES
    "${CMAKE_CURRENT_LIST_DIR}/trdp_bridge_common.c"
    "${CMAKE_CURRENT_LIST_DIR}/trdp_bridge_node.c"
    "${CMAKE_CURRENT_LIST_DIR}/trdp_bridge_capture.c"
    "${CMAKE_CURRENT_LIST_DIR}/trdp_bridge_main.c"
)

add_executable(tauterm-trdp-bridge ${BRIDGE_SOURCES})
target_include_directories(tauterm-trdp-bridge PRIVATE
    "${CMAKE_CURRENT_LIST_DIR}"
    "${TCNOPEN_API}"
    "${TCNOPEN_VOS_API}"
)
target_compile_definitions(tauterm-trdp-bridge PRIVATE ${TCNOPEN_PLATFORM_DEFINITIONS})
target_link_libraries(tauterm-trdp-bridge PRIVATE tauterm_tcnopen ${TCNOPEN_PLATFORM_LIBRARIES})

add_executable(trdp-test-peer
    "${CMAKE_CURRENT_LIST_DIR}/../../tools/trdp-test-peer/trdp_test_peer.c"
)
target_include_directories(trdp-test-peer PRIVATE
    "${TCNOPEN_API}"
    "${TCNOPEN_VOS_API}"
)
target_compile_definitions(trdp-test-peer PRIVATE ${TCNOPEN_PLATFORM_DEFINITIONS})
target_link_libraries(trdp-test-peer PRIVATE tauterm_tcnopen ${TCNOPEN_PLATFORM_LIBRARIES})

if(MSVC)
    target_compile_options(tauterm-trdp-bridge PRIVATE /W4 /WX)
    target_compile_options(trdp-test-peer PRIVATE /W4 /WX)
else()
    target_compile_options(tauterm-trdp-bridge PRIVATE -Wall -Wextra -Werror)
    target_compile_options(trdp-test-peer PRIVATE -Wall -Wextra -Werror)
endif()

set(CMAKE_RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")
foreach(config DEBUG RELEASE RELWITHDEBINFO MINSIZEREL)
    set(CMAKE_RUNTIME_OUTPUT_DIRECTORY_${config} "${CMAKE_BINARY_DIR}/bin")
endforeach()

if(TAUTERM_TRDP_SANITIZE AND NOT MSVC)
    set(TRDP_SANITIZER_FLAGS -fsanitize=address,undefined -fno-omit-frame-pointer -g)
    target_compile_options(tauterm_tcnopen PRIVATE ${TRDP_SANITIZER_FLAGS})
    target_compile_options(tauterm-trdp-bridge PRIVATE ${TRDP_SANITIZER_FLAGS})
    target_compile_options(trdp-test-peer PRIVATE ${TRDP_SANITIZER_FLAGS})
    target_link_options(tauterm-trdp-bridge PRIVATE -fsanitize=address,undefined)
    target_link_options(trdp-test-peer PRIVATE -fsanitize=address,undefined)
endif()
