#pragma once

#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <initializer_list>
#include <optional>
#include <stdexcept>
#include <utility>
#include <kiyosi/core/error.hpp>

namespace kiyosi {

enum class risk_measure : std::uint8_t {
    price,
    delta,
    gamma,
    speed,
    theta,
    charm,
    color,
    vega,
    vanna,
    zomma,
    rho,
};

inline constexpr std::size_t risk_measure_count = static_cast<std::size_t>(risk_measure::rho) + 1;

[[nodiscard]] constexpr std::optional<std::size_t> risk_measure_index(risk_measure measure) noexcept
{
    const auto index = static_cast<std::size_t>(measure);
    return index < risk_measure_count ? std::optional{index} : std::nullopt;
}

class PricingResult {
public:
    using values_type = std::array<std::optional<double>, risk_measure_count>;

    PricingResult() = default;
    PricingResult(std::initializer_list<std::pair<risk_measure, std::optional<double>>> entries)
    {
        for (const auto& [measure, value] : entries) {
            const auto index = risk_measure_index(measure);
            if (!index) throw std::invalid_argument{"unknown risk measure"};
            values_[*index] = value;
        }
    }

    [[nodiscard]] bool has(risk_measure measure) const noexcept
    {
        const auto index = risk_measure_index(measure);
        return index && values_[*index].has_value();
    }

    [[nodiscard]] result<std::optional<double>> get(risk_measure measure) const noexcept
    {
        const auto index = risk_measure_index(measure);
        if (!index)
            return std::unexpected(Error{error_category::invalid_parameter,
                                         "unknown risk measure"});
        return values_[*index];
    }

    [[nodiscard]] result<double> require(risk_measure measure) const
    {
        const auto value = get(measure);
        if (!value) return std::unexpected(value.error());
        if (*value) return **value;
        return std::unexpected(Error{error_category::invalid_result,
                                     "requested risk measure is unavailable"});
    }

    [[nodiscard]] const values_type& values_view() const noexcept { return values_; }

    [[nodiscard]] bool all_finite() const noexcept
    {
        for (const auto& value : values_)
            if (value && !std::isfinite(*value)) return false;
        return true;
    }

private:
    values_type values_{};
};

} // namespace kiyosi
