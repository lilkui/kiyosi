#pragma once

#include <catch2/catch_test_macros.hpp>

#include <chrono>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <map>
#include <string>

#include "support/reference_fixture.hpp"

namespace kiyosi::test {

inline const std::map<std::string, kiyosi::risk_measure> measures{
    {"price", kiyosi::risk_measure::price}, {"delta", kiyosi::risk_measure::delta},
    {"gamma", kiyosi::risk_measure::gamma}, {"speed", kiyosi::risk_measure::speed},
    {"theta", kiyosi::risk_measure::theta}, {"charm", kiyosi::risk_measure::charm},
    {"color", kiyosi::risk_measure::color}, {"vega", kiyosi::risk_measure::vega},
    {"vanna", kiyosi::risk_measure::vanna}, {"zomma", kiyosi::risk_measure::zomma},
    {"rho", kiyosi::risk_measure::rho}};

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

inline kiyosi::date standard_expiry()
{
    return kiyosi::date{std::chrono::year{2026} / 1 / 6};
}

/// Reads a scalar input column out of a fixture row.
inline double fixture_number(const ReferenceCase& fixture, const std::string& key)
{
    std::size_t index = 0;
    return detail::number({fixture.inputs.at(key)}, index, 0, key);
}

inline kiyosi::date fixture_date(const ReferenceCase& fixture, const std::string& key)
{
    std::size_t index = 0;
    return detail::calendar_date({fixture.inputs.at(key)}, index, 0, key);
}

template <typename PriceResult>
void check_price(const ReferenceCase& fixture, const PriceResult& priced)
{
    INFO("case=" << fixture.case_id << " instrument=" << fixture.instrument << " engine=" << fixture.engine);
    REQUIRE(priced.has_value());
    REQUIRE(fixture.outputs.contains("price"));
    REQUIRE(fixture.tolerances.contains("price"));
    REQUIRE(priced->require(kiyosi::risk_measure::price).has_value());
    CAPTURE(*priced->require(kiyosi::risk_measure::price));
    CHECK(std::abs(*priced->require(kiyosi::risk_measure::price) - fixture.outputs.at("price")) <=
          fixture.tolerances.at("price"));
}

inline const std::map<std::string, kiyosi::barrier_type> barrier_kinds{
    {"up_and_in", kiyosi::barrier_type::up_and_in},
    {"up_and_out", kiyosi::barrier_type::up_and_out},
    {"down_and_in", kiyosi::barrier_type::down_and_in},
    {"down_and_out", kiyosi::barrier_type::down_and_out}};

} // namespace kiyosi::test
