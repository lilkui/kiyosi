#include <kiyosi/market/rate_term_structure.hpp>

#include <algorithm>
#include <cmath>
#include <ranges>

namespace kiyosi {
namespace detail {
struct RateCurveNodes {
    std::vector<Timestamp> timestamps;
    std::vector<double> times;
    std::vector<double> factors;
    std::vector<double> forwards;
    friend bool operator==(const RateCurveNodes&, const RateCurveNodes&) = default;
};
} // namespace detail

Result<RateTermStructure> make_flat_rate(double rate, std::optional<Timestamp> reference_time)
{
    if (!std::isfinite(rate))
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "flat rate must be finite"});
    if (reference_time && !is_supported_date(date_of(*reference_time)))
        return std::unexpected(Error{ErrorCategory::invalid_date, "curve reference timestamp is unsupported"});
    return RateTermStructure{reference_time, rate};
}

Result<RateTermStructure> make_rate_term_structure(
    Timestamp reference_time, std::vector<Timestamp> times, std::vector<double> factors)
{
    if (!is_supported_date(date_of(reference_time)))
        return std::unexpected(Error{ErrorCategory::invalid_date, "curve reference timestamp is unsupported"});
    if (times.empty() || times.size() != factors.size())
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "curve nodes and factors must have the same nonzero size"});
    auto data = std::make_shared<detail::RateCurveNodes>();
    data->timestamps = std::move(times);
    data->factors = std::move(factors);
    data->times.reserve(data->timestamps.size());
    data->forwards.reserve(data->timestamps.size());
    Timestamp previous = reference_time;
    double previous_log = 0.0;
    for (std::size_t index = 0; index < data->timestamps.size(); ++index) {
        const Timestamp time = data->timestamps[index];
        if (!is_supported_date(date_of(time)))
            return std::unexpected(Error{ErrorCategory::invalid_date, "curve node timestamp is unsupported"});
        if (time <= previous || !std::isfinite(data->factors[index]) || data->factors[index] <= 0.0)
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "curve nodes must increase after the reference and factors must be finite and positive"});
        const double log_factor = std::log(data->factors[index]);
        const double forward = (previous_log - log_factor) / detail::actual_365_fixed_year_fraction(previous, time);
        if (!std::isfinite(forward))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "curve forward rate is not representable"});
        data->times.push_back(detail::actual_365_fixed_year_fraction(reference_time, time));
        data->forwards.push_back(forward);
        previous = time;
        previous_log = log_factor;
    }
    return RateTermStructure{reference_time, {}, std::move(data)};
}

std::optional<Timestamp> RateTermStructure::max_time() const noexcept
{
    return data_ ? std::optional{data_->timestamps.back()} : std::nullopt;
}

std::vector<Timestamp> RateTermStructure::nodes() const
{
    return data_ ? data_->timestamps : std::vector<Timestamp>{};
}

std::vector<double> RateTermStructure::factors() const
{
    if (!data_) return {};
    auto values = data_->factors;
    if (spread_ != 0.0)
        for (std::size_t index = 0; index < values.size(); ++index)
            values[index] = std::exp(std::log(values[index]) - spread_ * data_->times[index]);
    return values;
}

Result<void> RateTermStructure::validate_time(Timestamp time) const
{
    if (!is_supported_date(date_of(time)))
        return std::unexpected(Error{ErrorCategory::invalid_date, "curve query timestamp is unsupported"});
    if ((reference_time_ && time < *reference_time_) || (data_ && time > data_->timestamps.back()))
        return std::unexpected(Error{ErrorCategory::invalid_time_range, "timestamp is outside curve coverage"});
    return {};
}

Result<void> detail::RateTermStructureAccess::validate_horizon(
    const RateTermStructure& curve, Timestamp valuation, Timestamp end)
{
    if (auto valid = curve.validate_time(end); !valid) return valid;
    if (curve.is_flat()) return {};
    double previous = 0.0;
    Timestamp previous_time = valuation;
    for (auto node = std::ranges::upper_bound(curve.data_->timestamps, valuation);
         node != curve.data_->timestamps.end() && *node <= end; ++node) {
        const double time = actual_365_fixed_year_fraction(valuation, *node);
        if (time <= previous)
            return std::unexpected(Error{ErrorCategory::invalid_result, "curve knots are not distinct in pricing time"});
        previous = time;
        previous_time = *node;
    }
    if (previous_time < end && actual_365_fixed_year_fraction(valuation, end) <= previous)
        return std::unexpected(Error{ErrorCategory::invalid_result, "pricing horizon is not distinct from its preceding curve knot"});
    return {};
}

double RateTermStructure::forward_rate_unchecked(Timestamp origin, double time) const noexcept
{
    const auto found = std::ranges::upper_bound(data_->timestamps, time, {}, [origin](Timestamp node) {
        return detail::actual_365_fixed_year_fraction(origin, node);
    });
    const auto index = std::min(static_cast<std::size_t>(found - data_->timestamps.begin()), data_->timestamps.size() - 1);
    return data_->forwards[index] + spread_;
}

double RateTermStructure::log_factor_unchecked(Timestamp origin, double start, double end) const noexcept
{
    if (start == end) return 0.0;
    const auto found = std::ranges::upper_bound(data_->timestamps, start, {}, [origin](Timestamp node) {
        return detail::actual_365_fixed_year_fraction(origin, node);
    });
    std::size_t index = std::min(static_cast<std::size_t>(found - data_->timestamps.begin()), data_->timestamps.size() - 1);
    double result = -spread_ * (end - start);
    while (start < end) {
        const double next = index + 1 < data_->timestamps.size() ? std::min(end, detail::actual_365_fixed_year_fraction(origin, data_->timestamps[index])) : end;
        result -= data_->forwards[index] * (next - start);
        start = next;
        ++index;
    }
    return result;
}

Result<double> RateTermStructure::log_factor(Timestamp start, Timestamp end) const
{
    if (auto valid = validate_time(start); !valid) return std::unexpected(valid.error());
    if (auto valid = validate_time(end); !valid) return std::unexpected(valid.error());
    if (end < start)
        return std::unexpected(Error{ErrorCategory::invalid_time_range, "curve interval is reversed"});
    double value = 0.0;
    if (flat_rate_) {
        value = -*flat_rate_ * detail::actual_365_fixed_year_fraction(start, end);
    } else {
        // Subtract timestamps before converting so distant references cannot erase nearby nodes.
        auto index = std::min(static_cast<std::size_t>(std::ranges::upper_bound(data_->timestamps, start) - data_->timestamps.begin()), data_->timestamps.size() - 1);
        Timestamp current = start;
        value = -spread_ * detail::actual_365_fixed_year_fraction(start, end);
        while (current < end) {
            const Timestamp next = std::min(end, data_->timestamps[index]);
            value -= data_->forwards[index] * detail::actual_365_fixed_year_fraction(current, next);
            current = next;
            ++index;
        }
    }
    if (!std::isfinite(value))
        return std::unexpected(Error{ErrorCategory::invalid_result, "curve log factor is not finite"});
    return value;
}

Result<double> RateTermStructure::factor(Timestamp start, Timestamp end) const
{
    const auto log_value = log_factor(start, end);
    if (!log_value) return std::unexpected(log_value.error());
    const double value = std::exp(*log_value);
    if (!std::isfinite(value))
        return std::unexpected(Error{ErrorCategory::invalid_result, "curve factor is not finite"});
    return value;
}

Result<double> RateTermStructure::forward_rate(Timestamp time) const
{
    if (auto valid = validate_time(time); !valid) return std::unexpected(valid.error());
    if (flat_rate_) return *flat_rate_;
    const auto found = std::ranges::upper_bound(data_->timestamps, time);
    const auto index = std::min(static_cast<std::size_t>(found - data_->timestamps.begin()), data_->timestamps.size() - 1);
    return data_->forwards[index] + spread_;
}

Result<RateTermStructure> RateTermStructure::shifted(double shift) const
{
    if (!std::isfinite(shift))
        return std::unexpected(Error{ErrorCategory::invalid_parameter, "curve shift must be finite"});
    auto copy = *this;
    if (flat_rate_) {
        const double rate = *flat_rate_ + shift;
        if (!std::isfinite(rate))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "shifted flat rate is not finite"});
        copy.flat_rate_ = rate;
    } else {
        copy.spread_ += shift;
        if (!std::isfinite(copy.spread_) || std::ranges::any_of(data_->forwards, [&](double rate) {
                return !std::isfinite(rate + copy.spread_);
            }))
            return std::unexpected(Error{ErrorCategory::invalid_parameter, "shifted forward rate is not finite"});
    }
    return copy;
}

bool operator==(const RateTermStructure& left, const RateTermStructure& right)
{
    return left.reference_time_ == right.reference_time_ && left.flat_rate_ == right.flat_rate_ &&
           left.spread_ == right.spread_ &&
           (left.data_ == right.data_ || (left.data_ && right.data_ && *left.data_ == *right.data_));
}
} // namespace kiyosi
