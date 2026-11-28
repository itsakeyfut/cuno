#ifndef STATS_H
#define STATS_H

int sum(const int *values, int count);

/* The mean rounded down, and 0 for no values. */
int mean(const int *values, int count);

#endif
