# vcpkg's dav1d port (microsoft/vcpkg ports/dav1d at 43c031bb61, MIT) plus dav1d's own fix for MSVC 19.52
# (videolan/dav1d a34e0685b9, after 1.5.4), whose preprocessor rejects compat/msvc/stdatomic.h's
# __has_include_next test. The Arm runner's Visual Studio 18 has that compiler. Drop this overlay once a
# dav1d release with the fix is in vcpkg.
vcpkg_from_github(
    OUT_SOURCE_PATH SOURCE_PATH
    REPO videolan/dav1d
    REF "${VERSION}"
    SHA512 7ee5906640495919462b2242c44a8c3cc577fdda52fc25257792f6df919429d54e4a48c315a7a11759385f044f68d3d6573fd720853f447e2d8520a13693827f
    HEAD_REF master
    PATCHES
        msvc-has-include-next.patch
)

if (VCPKG_TARGET_ARCHITECTURE STREQUAL "x86" OR VCPKG_TARGET_ARCHITECTURE STREQUAL "x64")
    vcpkg_find_acquire_program(NASM)
    get_filename_component(NASM_EXE_PATH ${NASM} DIRECTORY)
    vcpkg_add_to_path(${NASM_EXE_PATH})
elseif (VCPKG_TARGET_IS_WINDOWS)
    vcpkg_find_acquire_program(GASPREPROCESSOR)
    foreach(GAS_PATH ${GASPREPROCESSOR})
        get_filename_component(GAS_ITEM_PATH ${GAS_PATH} DIRECTORY)
        vcpkg_add_to_path(${GAS_ITEM_PATH})
    endforeach(GAS_PATH)
endif()

set(LIBRARY_TYPE ${VCPKG_LIBRARY_LINKAGE})
if (LIBRARY_TYPE STREQUAL "dynamic")
    set(LIBRARY_TYPE "shared")
endif(LIBRARY_TYPE STREQUAL "dynamic")

vcpkg_configure_meson(
    SOURCE_PATH "${SOURCE_PATH}"
    OPTIONS
        --default-library=${LIBRARY_TYPE}
        -Denable_tests=false
        -Denable_tools=false
)

vcpkg_install_meson()
vcpkg_copy_pdbs()
vcpkg_fixup_pkgconfig()

file(INSTALL "${CMAKE_CURRENT_LIST_DIR}/usage" DESTINATION "${CURRENT_PACKAGES_DIR}/share/${PORT}")
vcpkg_install_copyright(
    FILE_LIST
        "${SOURCE_PATH}/COPYING"
        "${SOURCE_PATH}/src/ext/x86/x86inc.asm"
)
