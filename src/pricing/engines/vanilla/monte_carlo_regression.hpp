#pragma once

#include <array>
#include <cmath>

namespace kiyosi::detail {

using QuadraticRegressionMatrix = std::array<std::array<double, 4>, 3>;

#if defined(__CUDACC__)
#define KIYOSI_REGRESSION_HOST_DEVICE __host__ __device__
#else
#define KIYOSI_REGRESSION_HOST_DEVICE
#endif

KIYOSI_REGRESSION_HOST_DEVICE inline bool regression_isfinite(double value)
{
#if defined(__CUDA_ARCH__)
    return ::isfinite(value);
#else
    return std::isfinite(value);
#endif
}

template <typename Matrix, typename Coefficients>
KIYOSI_REGRESSION_HOST_DEVICE inline bool solve_quadratic(const Matrix& samples,
                                                          Coefficients& coefficients)
{
    // Reduce the basis when paths cannot identify a quadratic continuation value.
    for (int degree = 2; degree >= 0; --degree) {
#if defined(__CUDA_ARCH__)
        double matrix[3][4]; // NOLINT(modernize-avoid-c-arrays): device code cannot call host std::array methods.
#else
        QuadraticRegressionMatrix matrix;
#endif
        for (int row = 0; row < 3; ++row)
            for (int column = 0; column < 4; ++column)
                matrix[row][column] = samples[row][column];
        double scale = 0.0;
        for (int row = 0; row <= degree; ++row)
            for (int column = 0; column <= degree; ++column)
                if (std::abs(matrix[row][column]) > scale) scale = std::abs(matrix[row][column]);
        bool singular = false;
        for (int column = 0; column <= degree; ++column) {
            int pivot = column;
            for (int row = column + 1; row <= degree; ++row)
                if (std::abs(matrix[row][column]) > std::abs(matrix[pivot][column])) pivot = row;
            if (!regression_isfinite(matrix[pivot][column])) return false;
            if (std::abs(matrix[pivot][column]) <= 1e-14 * scale) {
                singular = true;
                break;
            }
            for (int entry = 0; entry < 4; ++entry) {
                const double value = matrix[column][entry];
                matrix[column][entry] = matrix[pivot][entry];
                matrix[pivot][entry] = value;
            }
            for (int row = column + 1; row <= degree; ++row) {
                const double factor = matrix[row][column] / matrix[column][column];
                for (int entry = column; entry <= degree; ++entry)
                    matrix[row][entry] -= factor * matrix[column][entry];
                matrix[row][3] -= factor * matrix[column][3];
            }
        }
        if (singular) continue;
        for (int index = 0; index < 3; ++index)
            coefficients[index] = 0.0;
        for (int row = degree; row >= 0; --row) {
            double value = matrix[row][3];
            for (int column = row + 1; column <= degree; ++column)
                value -= matrix[row][column] * coefficients[column];
            coefficients[row] = value / matrix[row][row];
        }
        return regression_isfinite(coefficients[0]) && regression_isfinite(coefficients[1]) &&
               regression_isfinite(coefficients[2]);
    }
    return false;
}

#undef KIYOSI_REGRESSION_HOST_DEVICE

} // namespace kiyosi::detail
