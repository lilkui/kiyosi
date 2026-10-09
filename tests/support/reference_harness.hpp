#pragma once

#include <catch2/catch_test_macros.hpp>
#include <catch2/matchers/catch_matchers_floating_point.hpp>

#include <cmath>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <map>
#include <string>

#include "support/reference_fixture.hpp"

namespace kiyosi::test {

// NOLINTNEXTLINE(bugprone-throwing-static-initialization): test fixture lookup.
inline const std::map<std::string, kiyosi::Greek> measures{
    {"delta", kiyosi::Greek::delta}, {"gamma", kiyosi::Greek::gamma}, {"speed", kiyosi::Greek::speed}, {"theta", kiyosi::Greek::theta}, {"charm", kiyosi::Greek::charm}, {"color", kiyosi::Greek::color}, {"vega", kiyosi::Greek::vega}, {"vanna", kiyosi::Greek::vanna}, {"zomma", kiyosi::Greek::zomma}, {"rho", kiyosi::Greek::rho}};

inline std::filesystem::path fixture_path()
{
    return std::filesystem::path{KIYOSI_SOURCE_DIR} / "tests" / "fixtures" / "pricing_reference.tsv";
}

inline std::string fixture_text()
{
    std::ifstream input(fixture_path());
    REQUIRE(input.good());
    return {std::istreambuf_iterator<char>{input}, std::istreambuf_iterator<char>{}};
}

/// Reads a scalar input column out of a fixture row.
inline double fixture_number(const ReferenceCase& fixture, const std::string& key)
{
    return detail::number(fixture.inputs.at(key), 0, key);
}

inline kiyosi::Date fixture_date(const ReferenceCase& fixture, const std::string& key)
{
    return detail::calendar_date(fixture.inputs.at(key), 0, key);
}

template <typename PriceResult>
void check_price(const ReferenceCase& fixture, const PriceResult& priced)
{
    INFO("case=" << fixture.case_id << " instrument=" << fixture.instrument << " engine=" << fixture.engine);
    REQUIRE(priced.has_value());
    REQUIRE(fixture.outputs.contains("price"));
    REQUIRE(fixture.tolerances.contains("price"));
    const double value = [&] {
        if constexpr (std::is_same_v<typename PriceResult::value_type, double>) return *priced;
        else return priced->price();
    }();
    CAPTURE(value);
    CHECK(std::abs(value - fixture.outputs.at("price")) <=
          fixture.tolerances.at("price"));
}

inline void check_numerical_result(const ReferenceCase& fixture, const kiyosi::Result<kiyosi::PricingResult>& priced)
{
    REQUIRE(priced.has_value());
    CHECK_THAT(priced->price(), Catch::Matchers::WithinAbs(
                                    fixture.outputs.at("price"), fixture_number(fixture, "numerical_tolerance_price") + fixture_number(fixture, "uncertainty_price")));
    for (const auto& [name, measure] : measures) {
        INFO("measure=" << name);
        REQUIRE(fixture.outputs.contains(name));
        REQUIRE(priced->has(measure));
        CHECK_THAT(*priced->require(measure),
                   Catch::Matchers::WithinAbs(fixture.outputs.at(name),
                                              fixture_number(fixture, "numerical_tolerance_" + name) +
                                                  fixture_number(fixture, "uncertainty_" + name)));
    }
}

// NOLINTNEXTLINE(bugprone-throwing-static-initialization): test fixture lookup.
inline const std::map<std::string, kiyosi::BarrierType> barrier_kinds{
    {"up_and_in", kiyosi::BarrierType::up_and_in},
    {"up_and_out", kiyosi::BarrierType::up_and_out},
    {"down_and_in", kiyosi::BarrierType::down_and_in},
    {"down_and_out", kiyosi::BarrierType::down_and_out}};

} // namespace kiyosi::test
