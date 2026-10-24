#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <numbers>

#include "pricing/engines/monte_carlo_sobol.hpp"

using Catch::Matchers::WithinAbs;

TEST_CASE("Scrambled Sobol points match independent SciPy direction and LMS references", "[sobol-sequence]")
{
    // SciPy 1.18.1 _initialize_v/_cscramble, 52 bits, PCG32 stream per dimension, seed 42.
    constexpr std::array<std::array<double, 3>, 8> expected{{
        {0x1.d5bbb057bdd42p-2, 0x1.ccf9e58387528p-4, 0x1.7b32537934bdap-2},
        {0x1.e7ed1e4c44f87p-1, 0x1.06a50e318ad1bp-1, 0x1.cfca62e11c7c1p-1},
        {0x1.2fe5ba5228bf9p-1, 0x1.0a7c93ce474a6p-2, 0x1.cd769f871eb84p-3},
        {0x1.16abe1ad956f8p-4, 0x1.ba047b66d99edp-1, 0x1.010eecbc418cdp-1},
        {0x1.072a4501d15f4p-3, 0x1.cb95d968337a6p-2, 0x1.bf67879d5b2f3p-1},
        {0x1.4cfa5727ee45bp-1, 0x1.daf0de35e386dp-1, 0x1.9a699981ba1bep-2},
        {0x1.84f2f33982025p-1, 0x1.65ae678d2bc94p-3, 0x1.71a309c006dffp-1},
        {0x1.13846abc30206p-2, 0x1.6651ab62b0c9bp-1, 0x1.f8214ec07e980p-8},
    }};
    const kiyosi::detail::SobolSequence sequence{3, 42};
    kiyosi::detail::SobolCursor cursor{sequence};
    for (const auto& point : expected) {
        for (const double coordinate : point)
            CHECK(cursor.uniform() == coordinate);
        cursor.next_path();
    }
}

TEST_CASE("Sobol random access matches sequential cursors across block boundaries", "[sobol-sequence]")
{
    const kiyosi::detail::SobolSequence sequence{7, 0xfedcba9876543210ULL};
    kiyosi::detail::SobolCursor sequential{sequence};
    for (std::uint64_t index = 0; index <= 1024; ++index) {
        kiyosi::detail::SobolCursor located{sequence, index};
        for (std::size_t dimension = 0; dimension < sequence.dimensions(); ++dimension)
            CHECK(located.uniform() == sequential.uniform());
        sequential.next_path();
    }
}

TEST_CASE("Sobol cursors reset dimensions after an early terminated path", "[sobol-sequence]")
{
    const kiyosi::detail::SobolSequence sequence{3, 42};
    kiyosi::detail::SobolCursor cursor{sequence};
    CHECK(cursor.uniform() == 0x1.d5bbb057bdd42p-2);
    cursor.next_path();
    CHECK(cursor.uniform() == 0x1.e7ed1e4c44f87p-1);
    CHECK(cursor.uniform() == 0x1.06a50e318ad1bp-1);
    cursor.next_path();
    CHECK(cursor.uniform() == 0x1.2fe5ba5228bf9p-1);
}

TEST_CASE("Scrambled Sobol preserves one and two dimensional digital net stratification", "[sobol-sequence]")
{
    const kiyosi::detail::SobolSequence sequence{8, 123456789};
    kiyosi::detail::SobolCursor cursor{sequence};
    std::array<std::array<int, 1024>, 8> bins{};
    std::array<int, 256> pairs{};
    for (int point = 0; point < 1024; ++point) {
        std::array<std::size_t, 2> pair{};
        for (std::size_t dimension = 0; dimension < sequence.dimensions(); ++dimension) {
            const double value = cursor.uniform();
            REQUIRE(value > 0.0);
            REQUIRE(value < 1.0);
            ++bins[dimension][static_cast<std::size_t>(value * 1024.0)];
            if (point < 256 && dimension < 2)
                pair[dimension] = static_cast<std::size_t>(value * 16.0);
        }
        if (point < 256)
            ++pairs[pair[0] * 16 + pair[1]];
        cursor.next_path();
    }
    for (const auto& dimension : bins)
        for (const int count : dimension)
            CHECK(count == 1);
    for (const int count : pairs)
        CHECK(count == 1);
}

TEST_CASE("Sobol coordinate prefixes are independent of dimension count and use all seed bits", "[sobol-sequence]")
{
    const kiyosi::detail::SobolSequence short_sequence{2, 7};
    const kiyosi::detail::SobolSequence long_sequence{9, 7};
    const kiyosi::detail::SobolSequence different_seed{2, (std::uint64_t{1} << 32U) | 7U};
    kiyosi::detail::SobolCursor short_cursor{short_sequence};
    kiyosi::detail::SobolCursor long_cursor{long_sequence};
    kiyosi::detail::SobolCursor different_cursor{different_seed};
    CHECK(short_cursor.uniform() != different_cursor.uniform());
    short_cursor = kiyosi::detail::SobolCursor{short_sequence};
    for (int point = 0; point < 32; ++point) {
        for (int dimension = 0; dimension < 2; ++dimension)
            CHECK(short_cursor.uniform() == long_cursor.uniform());
        short_cursor.next_path();
        long_cursor.next_path();
    }
}

TEST_CASE("Joe Kuo high dimensions match independent reference points at the path limit", "[sobol-sequence]")
{
    constexpr std::array<std::uint64_t, 5> indices{1, 255, 256, 65535, 8388607};
    constexpr std::array<std::array<double, 3>, 5> expected{{
        {0x1.e7ed1e4c44f87p-1, 0x1.89cad99661532p-2, 0x1.b82c3584301d7p-1},
        {0x1.d01425a89c7e6p-2, 0x1.e517df11100c7p-1, 0x1.4453a3328d149p-1},
        {0x1.d35f723d579bap-2, 0x1.a8a91e5667531p-1, 0x1.8a16ae03f038ap-2},
        {0x1.d5bf4b9d8944ap-2, 0x1.9fdd5019b3890p-5, 0x1.740919ce3c3f6p-2},
        {0x1.d5bbbd784bb6ap-2, 0x1.05b727e42f3e4p-3, 0x1.da38b73118a56p-2},
    }};
    constexpr std::array<std::size_t, 3> dimensions{0, 9999, 21200};
    const kiyosi::detail::SobolSequence sequence{21201, 42};
    for (std::size_t point = 0; point < indices.size(); ++point) {
        kiyosi::detail::SobolCursor cursor{sequence, indices[point]};
        std::size_t selected = 0;
        for (std::size_t dimension = 0; dimension < sequence.dimensions(); ++dimension) {
            const double value = cursor.uniform();
            if (selected < dimensions.size() && dimension == dimensions[selected]) {
                CHECK(value == expected[point][selected]);
                ++selected;
            }
        }
    }
}

TEST_CASE("Inverse normal matches independent tail and central quantile references", "[sobol-sequence]")
{
    // SciPy 1.18.1 special.ndtri reference values, including both midpoint endpoints.
    constexpr std::array<std::array<double, 2>, 12> references{{
        {0x1p-53, -0x1.06b48528cea52p+3},
        {0x1.6849b86a12b9bp-47, -0x1.e9a3e40341036p+2},
        {0x1.b7cdfd9d7bdbbp-34, -0x1.97203597a2154p+2},
        {0x1.47ae147ae147bp-7, -0x1.29c5c4630ff0ep+1},
        {0x1.152aaa3bf81ccp-3, -0x1.19fd30bc4de02p+0},
        {0x1p-2, -0x1.5956b87528a49p-1},
        {0x1p-1, 0.0},
        {0x1.8p-1, 0x1.5956b87528a49p-1},
        {0x1.bab5557101f8dp-1, 0x1.19fd30bc4de03p+0},
        {0x1.fae147ae147aep-1, 0x1.29c5c4630ff0ep+1},
        {0x1.ffffffff24190p-1, 0x1.97203589fd4f6p+2},
        {0x1.fffffffffffffp-1, 0x1.06b48528cea52p+3},
    }};
    for (const auto& [probability, expected] : references) {
        const double value = kiyosi::detail::inverse_normal_cdf(probability);
        REQUIRE(std::isfinite(value));
        CHECK_THAT(value, WithinAbs(expected, 3e-15));
    }
    double previous = kiyosi::detail::inverse_normal_cdf(0x1p-53);
    for (int index = 0; index < 2048; ++index) {
        const double probability = (static_cast<double>(index) + 0.5) / 2048.0;
        const double value = kiyosi::detail::inverse_normal_cdf(probability);
        CHECK(value > previous);
        CHECK_THAT(0.5 * std::erfc(-value / std::numbers::sqrt2), WithinAbs(probability, 4e-16));
        previous = value;
    }
}
