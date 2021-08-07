/*
 * cache.c - guess the cache size(s)
 *
 * usage: cache [-c] [-L <line size>] [-M len[K|M]] [-W <warmup>] [-N <repetitions>]
 *
 * Copyright (c) 2000 Carl Staelin.
 * Copyright (c) 1994 Larry McVoy.  Distributed under the FSF GPL with
 * additional restriction that results may published only if
 * (1) the benchmark is unmodified, and
 * (2) the version in the sccsid below is included in the report.
 * Support for this development by Sun Microsystems is gratefully acknowledged.
 */
char	*id = "$Id$\n";

#include "bench.h"


struct cache_results {
	size_t	len;
	size_t	maxlen;
	size_t	line;
	double	latency;
	double	variation;
	double	ratio;
	double	slope;
};

int	find_cache(int start, int n, double prev_lat, struct cache_results* p);
int	collect_data(size_t start, size_t line, size_t maxlen, 
		     int repetitions, struct cache_results** pdata);
void	search(int left, int right, int repetitions, 
	       struct mem_state* state, struct cache_results* p);
int	collect_sample(int repetitions, struct mem_state* state, 
			struct cache_results* p);
double	measure(size_t size, int repetitions, 
		double* variation, struct mem_state* state);
double	remove_chunk(size_t i, size_t chunk, size_t npages, size_t* pages, 
		       size_t len, int repetitions, struct mem_state* state);
int	test_chunk(size_t i, size_t chunk, size_t npages, size_t* pages, 
		   size_t len, double *baseline, double chunk_baseline,
		   int repetitions, struct mem_state* state);
int	fixup_chunk(size_t i, size_t chunk, size_t npages, size_t* pages, 
		    size_t len, double *baseline, double chunk_baseline,
		    int repetitions, struct mem_state* state);
void	check_memory(size_t size, struct mem_state* state);
void	pagesort(size_t n, size_t* pages, double* latencies);

#ifdef ABS
#undef ABS
#endif
#define ABS(a) ((a) < 0 ? -(a) : (a))

#define SWAP(a,b) {size_t _tmp = (a); (a) = (b); (b) = _tmp;}

#define THRESHOLD 1.5

#define	FIVE(m)		m m m m m
#define	TEN(m)		FIVE(m) FIVE(m)
#define	FIFTY(m)	TEN(m) TEN(m) TEN(m) TEN(m) TEN(m)
#define	HUNDRED(m)	FIFTY(m) FIFTY(m)
#define DEREF		p = (char**)*p;

static char **addr_save = NULL;

void
mem_benchmark(iter_t iterations, void *cookie)
{
	register char **p;
	struct mem_state* state = (struct mem_state*)cookie;

	p = addr_save ? addr_save : (char**)state->p[0];
	while (iterations-- > 0) {
		HUNDRED(DEREF);
	}
	addr_save = p;
}


/*
 * Assumptions:
 *
 * 1) Cache lines are a multiple of pointer-size words
 * 2) Cache lines are no larger than 1/8 of a page (typically 512 bytes)
 * 3) Pages are an even multiple of cache lines
 */
int
main(int ac, char **av)
{
	int	c;
	int	i, j, n, start, level, prev, min;
	int	warmup = 0;
	int	repetitions = (1000000 <= get_enough(0) ? 1 : TRIES);
	ssize_t	line = 0;
	size_t	maxlen = 32 * 1024 * 1024;
	int	*levels;
	double	par, maxpar, prev_lat;
	char   *usage = "[-c] [-L <line size>] [-M len[K|M]] [-W <warmup>] [-N <repetitions>]\n";
	struct cache_results* r;
	struct mem_state state;

	while (( c = getopt(ac, av, "L:M:W:N:")) != EOF) {
		switch(c) {
		case 'L':
			line = atoi(optarg);
			if (line < sizeof(char*))
				line = sizeof(char*);
			break;
		case 'M':
			maxlen = bytes(optarg);
			break;
		case 'W':
			warmup = atoi(optarg);
			break;
		case 'N':
			repetitions = atoi(optarg);
			break;
		default:
			lmbench_usage(ac, av, usage);
			break;
		}
	}

	sched_pin(0);

	state.width = 1;
	state.len = maxlen;
	state.maxlen = maxlen;
	state.pagesize = getpagesize();

	if (line == 0) {
		line = line_find(maxlen, warmup, repetitions, &state);
		if (line == 0)
			line = getpagesize() / 16;
		state.line = line;
	}

	n = collect_data((size_t)512, line, maxlen, repetitions, &r);
	r[n-1].line = line;
	levels = (int*)malloc(n * sizeof(int));
	if (!levels) {
		perror("malloc");
		exit(1);
	}
	bzero(levels, n * sizeof(int));

	for (start = 0, prev = 0, level = 0, prev_lat = -1.0; 
	     (i = find_cache(start, n, prev_lat, r)) >= 0; 
	     ++level, start = i + 1, prev = i) 
	{
		/* 
		 * performance is not greatly improved over main memory,
		 * so it is likely not a cache boundary
		 */
		if (r[i].latency / r[n-1].latency > 0.5) break;

		/* 
		 * is cache boundary "legal"? (e.g. 2^N or 1.5*2^N) 
		 * cache sizes are "never" 1.25*2^N or 1.75*2^N
		 */
		for (c = r[i].len; c > 0x7; c >>= 1)
			;
		if (c == 5 || c == 7) {
			i++;
			if (i >= n) break;
		}

		levels[level] = i;
		prev_lat = (r[start].latency > 0.0 ? r[start].latency : r[start - 1].latency);
	}

	for (i = 0; i < level; ++i) {
		prev = (i > 0 ? levels[i-1]: -1);

		/* locate most likely cache latency */
		for (j = min = prev + 1; j < levels[i]; ++j) {
			if (r[j].latency <= 0.) continue;
			if (r[min].latency <= 0.
			    || ABS(r[j].slope) < ABS(r[min].slope)) {
				min = j;
			}
		}

		/* Compute line size */
		if (i == level - 1) {
			line = r[n-1].line;
		} else {
			j = (levels[i] + levels[i+1]) / 2;
			for (line = -1; line <= 0 && j < n; ++j) {
				r[j].line = line_find(r[j].len, warmup,
						      repetitions, &state);
				line = r[j].line;
			}
		}

		/* Compute memory parallelism for cache */
		maxpar = par_mem(r[levels[i]-1].len, warmup, 
				 repetitions, &state);

		fprintf(stderr, 
		    "L%d cache: %lu bytes %.2f nanoseconds %ld linesize %.2f parallelism\n",
		    (int)(i+1), (unsigned long)r[levels[i]].len, 
		    r[min].latency, (long)line, maxpar);
	}

	/* Compute memory parallelism for main memory */
	j = n - 1;
	for (i = n - 1; i >= 0; i--) {
		if (r[i].latency < 0.) continue;
		if (r[i].latency > 0.99 * r[n-1].latency)
			j = i;
	}
	par = par_mem(r[j].len, warmup, repetitions, &state);

	fprintf(stderr, "Memory latency: %.2f nanoseconds %.2f parallelism\n",
		r[n-1].latency, par);

	exit(0);
}

int
find_cache(int start, int n, double prev_lat, struct cache_results* p)
{
	int	i, j, prev;
	double	max = -1.;

	for (prev = (start == 0 ? start : start - 1); prev > 0; prev--) {
		if (p[prev].ratio > 0.0) break;
	}

	for (i = start, j = -1; i < n; ++i) {
		if (p[i].latency < 0.)
			continue;
		if (max < p[i].ratio)
			max = p[i].ratio;
		if (THRESHOLD < p[i].ratio)
			j = i;
		if (THRESHOLD < max && p[j].len * 2 <= p[i].len)
			return j;
		prev = i;
	}
	return -1;
}

int
collect_data(size_t start, size_t line, size_t maxlen, 
	     int repetitions, struct cache_results** pdata)
{
	int	i;
	int	samples;
	int	idx;
	size_t	len = start;
	size_t	incr = start / 4;
	struct mem_state state;
	struct cache_results* p;


	state.width = 1;
	state.len = maxlen;
	state.maxlen = maxlen;
	state.line = line;
	state.pagesize = getpagesize();
	state.addr = NULL;

	/* count the (maximum) number of samples to take */
	for (len = start, incr = start / 4, samples = 0; len <= maxlen; incr<<=1) {
		for (i = 0; i < 4 && len <= maxlen; ++i, len += incr)
			samples++;
	}
	*pdata = (struct cache_results*)
		malloc(samples * sizeof(struct cache_results));
	if (!*pdata) {
		perror("malloc");
		exit(2);
	}
	p = *pdata;

	/* initialize the data */
	for (len = start, incr = start / 4, idx = 0; len <= maxlen; incr<<=1) {
		for (i = 0; i < 4 && len <= maxlen; ++i, ++idx, len += incr) {
			p[idx].len = len;
			p[idx].line = line;
			p[idx].latency = -1.;
			p[idx].ratio = -1.;
			p[idx].slope = -1.;
		}
	}

	/* make sure we have enough memory for the scratch data */
	while (state.addr == NULL) {
		mem_initialize(0, &state);
		if (state.addr == NULL) {
			maxlen /= 2;
			state.len = state.maxlen = maxlen;
			while (p[samples-1].len > maxlen)
				samples--;
		}
	}
	for (i = 0; i < samples; ++i)
		p[i].maxlen = maxlen;
	/* in case the system has laid out the pages well, don't scramble */
	for (i = 0; i < state.npages; ++i)
		state.pages[i] = i * state.pagesize;

	p[samples-1].latency = measure(p[samples-1].len, repetitions, 
				       &p[samples-1].variation, &state);
	while (p[samples-1].latency <= 0.0) {
		p[samples-1].latency = measure(p[samples-1].len, 
					       repetitions, 
					       &p[samples-1].variation, 
					       &state);
		--samples;
	}
	p[0].latency = measure(p[0].len, repetitions, &p[0].variation, &state);
	search(0, samples - 1, repetitions, &state, p);

	/*
	fprintf(stderr, "%10.10s %8.8s %8.8s %8.8s %8.8s %5.5s\n", 
		"mem size", "latency", "variation", "ratio", "slope", "line");
	for (idx = 0; idx < samples; ++idx) {
		if (p[idx].latency < 0.) continue;
		fprintf(stderr, 
			"%10.6f %8.3f %8.3f %8.3f %8.3f %4lu\n", 
			p[idx].len / (1000. * 1000.), 
			p[idx].latency, 
			p[idx].variation, 
			p[idx].ratio,
			p[idx].slope,
			(unsigned long)p[idx].line);
	}
	/**/
	mem_cleanup(0, &state);

	return samples;
}

void
search(int left, int right, int repetitions, 
       struct mem_state* state, struct cache_results* p)
{
	int	middle = left + (right - left) / 2;

	/*
	fprintf(stderr, 
		"search(%d, %d, ...): [%lu/%G, %lu, %lu/%G] entering\n",
		left, right, 
		(unsigned long)p[left].len, p[left].latency, 
		(unsigned long)p[middle].len, 
		(unsigned long)p[right].len, p[right].latency);
	/**/

	if (p[left].latency > 0.0) {
		p[left].ratio = p[right].latency / p[left].latency;
		p[left].slope = (p[left].ratio - 1.) / (double)(right - left);
		/* we probably have a bad data point, so ignore it */
		if (p[left].ratio < 0.98) {
			p[left].latency = p[right].latency;
			p[left].ratio = 1.;
			p[left].slope = 0.;
		}
	}

	if (middle == left || middle == right)
		return;

	if (p[left].ratio > 1.35 || p[left].ratio < 0.97) {
		collect_sample(repetitions, state, &p[middle]);
		search(middle, right, repetitions, state, p);
		search(left, middle, repetitions, state, p);
	}
	return;
}

int
collect_sample(int repetitions, struct mem_state* state, 
	       struct cache_results* p)
{
	int	i, modified, npages;
	double	baseline;

	npages = (p->len + getpagesize() - 1) / getpagesize();
        baseline = measure(p->len, repetitions, &p->variation, state);
	
	if (npages > 1) {
		for (i = 0, modified = 1; i < 8 && modified; ++i) {
			modified = test_chunk(0, npages, npages, 
					      state->pages, p->len, 
					      &baseline, 0.0,
					      repetitions, state);
		}
	}
	p->latency = baseline;

	/*
	fprintf(stderr, "collect_sample: len=%lu, latency=%G\n", 
		(unsigned long)p->len, p->latency);
	/**/

	return (p->latency > 0);
}

double
measure(size_t size, int repetitions, 
	double* variation, struct mem_state* state)
{
	size_t	i, j, npages, nlines;
	double	time, median;
	char	*p;
	result_t *r, *r_save;
	size_t	*pages;

	pages = state->pages;
	npages = (size + getpagesize() - 1) / getpagesize();
	nlines = state->nlines;

	if (size % getpagesize())
		nlines = (size % getpagesize()) / state->line;

	r_save = get_results();
	r = (result_t*)malloc(sizeof_result(repetitions));
	if (!r) {
		perror("malloc");
		exit(3);
	}
	insertinit(r);

	/* 
	 * assumes that you have used mem_initialize() to setup the memory
	 */
	p = state->base;
	for (i = 0; i < npages - 1; ++i) {
		for (j = 0; j < state->nwords; ++j) {
			*(char**)(p + pages[i] + state->lines[state->nlines - 1] + state->words[j]) = 
			p + pages[i+1] + state->lines[0] + state->words[j];
		}
	}
	for (j = 0; j < state->nwords; ++j) {
		*(char**)(p + pages[npages - 1] + state->lines[nlines - 1] + state->words[j]) = 
			p + pages[0] + state->lines[0] + state->words[(j+1)%state->nwords];
	}

	/*
	check_memory(size, state);
	/**/

	addr_save = NULL;
	state->p[0] = p + pages[0] + state->lines[0] + state->words[0];
	/* now, run through the chain once to clear the cache */
	mem_benchmark((size / sizeof(char*) + 100) / 100, state);

	for (i = 0; i < repetitions; ++i) {
		BENCH1(mem_benchmark(__n, state); __n = 1;, 0)
		insertsort(gettime(), get_n(), r);
	}
	set_results(r);
	median = (1000. * (double)gettime()) / (100. * (double)get_n());

	save_minimum();
	time = (1000. * (double)gettime()) / (100. * (double)get_n());

	/* Are the results stable, or do they vary? */
	if (time != 0.)
		*variation = median / time;
	else
		*variation = -1.0;
	set_results(r_save);
	free(r);

	if (nlines < state->nlines) {
		for (j = 0; j < state->nwords; ++j) {
			*(char**)(p + pages[npages - 1] + state->lines[nlines - 1] + state->words[j]) = 
				p + pages[npages - 1] + state->lines[nlines] + state->words[j];
		}
	}
	/*
	fprintf(stderr, "%.6f %.2f\n", size / (1000. * 1000.), median);
	/**/

	return median;
}


double
remove_chunk(size_t i, size_t chunk, size_t npages, size_t* pages, 
	     size_t len, int repetitions, struct mem_state* state)
{
	size_t	n, j;
	double	t, var;

	if (i + chunk < npages) {
		for (j = 0; j < chunk; ++j) {
			n = pages[i+j];
			pages[i+j] = pages[npages-1-j];
			pages[npages-1-j] = n;
		}
	}
	t = measure(len - chunk * getpagesize(), repetitions, &var, state);
	if (i + chunk < npages) {
		for (j = 0; j < chunk; ++j) {
			n = pages[i+j];
			pages[i+j] = pages[npages-1-j];
			pages[npages-1-j] = n;
		}
	}
	
	return t;
}

int
test_chunk(size_t i, size_t chunk, size_t npages, size_t* pages, 
	   size_t len, double *baseline, double chunk_baseline,
	   int repetitions, struct mem_state* state)
{
	int	modified = 0;
	int	changed;
	size_t	j, k, subchunk;
	double	t, tt, nodiff_chunk_baseline;

	if (chunk <= 20 && chunk < npages) {
		return fixup_chunk(i, chunk, npages, pages, len, baseline, 
				   chunk_baseline, repetitions, state);
	}

	nodiff_chunk_baseline = *baseline;
	subchunk = (chunk + 19) / 20;
	for (j = i, k = 0; j < i + chunk; j+=subchunk, k++) {
		if (j + subchunk > i + chunk) subchunk = i + chunk - j;

		t = remove_chunk(j, subchunk, npages, pages, 
				 len, repetitions, state);

		/*
		fprintf(stderr, "test_chunk(...): baseline=%G, t=%G, len=%d, chunk=%d, i=%d\n", *baseline, t, len, subchunk, j);
		/**/

		if (t >= 0.99 * *baseline) continue;
		if (t >= 0.999 * nodiff_chunk_baseline) continue;

		tt = remove_chunk(j, subchunk, npages, pages, 
				  len, repetitions, state);

		if (tt > t) t = tt;

		if (t >= 0.99 * *baseline) continue;
		if (t >= 0.999 * nodiff_chunk_baseline) continue;

		changed = test_chunk(j, subchunk, npages, pages, len,
				     baseline, t, repetitions, state);

		if (changed) {
			modified = 1;
		} else {
			nodiff_chunk_baseline = t;
		}
	}
	return modified;
}

/*
 * This routine is called once we have identified a chunk
 * that has pages that are suspected of colliding with other
 * pages.
 *
 * The algorithm is to remove all the pages, and then 
 * slowly add back pages; attempting to add pages with
 * minimal cost.
 */
int
fixup_chunk(size_t i, size_t chunk, size_t npages, size_t* pages, 
	    size_t len, double *baseline, double chunk_baseline,
	    int repetitions, struct mem_state* state)
{
	int	swapped = 0;
	size_t	j, k;
	size_t	page, substitute, original;
	size_t	ntotalpages, nsparepages;
	size_t	subset_len;
	size_t	*pageset;
	size_t	*saved_pages;
	static size_t	available_index = 0;
	double	t, var, new_baseline;
	double	latencies[20];

	ntotalpages = (state->maxlen + getpagesize() - 1)/ getpagesize();
	nsparepages = ntotalpages - npages;
	pageset = state->pages + npages;
	new_baseline = *baseline;

	saved_pages = (size_t*)malloc(sizeof(size_t) * ntotalpages);
	if (!saved_pages) {
		perror("malloc");
		exit(4);
	}
	bcopy(pages, saved_pages, sizeof(int) * ntotalpages);

	/* move everything to the end of the page list */
	if (i + chunk < npages) {
		for (j = 0; j < chunk; ++j) {
			page = pages[i+j];
			pages[i+j] = pages[npages-chunk+j];
			pages[npages-chunk+j] = page;
		}
	}

	if (available_index >= nsparepages) available_index = 0;

	/* 
	 * first try to identify which pages we can definitely keep
	 */
	for (j = 0, k = chunk; j < k; ) {

		t = measure((npages - chunk + j + 1) * getpagesize(), 
			    repetitions, &var, state);

		if (0.995 * t <= chunk_baseline) {
			latencies[j] = t;
			++j;	/* keep this page */
		} else {	
			--k;	/* this page is probably no good */
			latencies[k] = t;
			SWAP(pages[npages - chunk + j], pages[npages - chunk + k]);
		}
	}
	/*
	 * sort the "bad" pages by increasing latency
	 */
	pagesort(chunk - j, &pages[npages - chunk + j], &latencies[j]);

	/*
	fprintf(stderr, "fixup_chunk: len=%d, chunk=%d, j=%d, baseline=%G, lat[%d]=%G..%G\n", len, chunk, j, *baseline, j, (j < chunk ? latencies[j] : -1.0), latencies[chunk - 1]);
	/**/

	if (chunk >= npages && j < chunk / 2) {
		j = chunk / 2;
		t = measure((npages - chunk + j + 1) * getpagesize(), 
			    repetitions, &var, state);
		chunk_baseline = t;
	}

	for (k = 0; j < chunk && k < 2 * npages; ++k) {
		original = npages - chunk + j;
		substitute = nsparepages - 1;
		substitute -= (k + available_index) % (nsparepages - 1);
		subset_len = (original + 1) * getpagesize();
		if (j == chunk - 1 && len % getpagesize()) {
			subset_len = len;
		}
		
		SWAP(pages[original], pageset[substitute]);
		t = measure(subset_len, repetitions, &var, state);
		SWAP(pages[original], pageset[substitute]);

		/*
		 * try to keep pages ordered by increasing latency
		 */
		if (t < latencies[chunk - 1]) {
			latencies[chunk - 1] = t;
			SWAP(pages[npages - 1], pageset[substitute]);
			pagesort(chunk - j, 
				 &pages[npages - chunk + j], &latencies[j]);
		}
		if (0.995 * latencies[j] <= chunk_baseline) {
			++j;	/* keep this page */
			++swapped;
		}
	}
				
	available_index = (k + available_index) % (nsparepages - 1);

	/* measure new baseline, in case we didn't manage to optimally
	 * replace every page
	 */
	if (swapped) {
		new_baseline = measure(len, repetitions, &var, state);

		/*
		fprintf(stderr, "fixup_chunk: len=%d, swapped=%d, k=%d, baseline=%G, newbase=%G\n", len, swapped, k, *baseline, new_baseline);
		/**/

		if (new_baseline >= 0.999 * *baseline) {
			/* no benefit to these changes; back them out */
			swapped = 0;
			bcopy(saved_pages, pages, sizeof(int) * ntotalpages);
		} else {
			/* we sped up, so keep these changes */
			*baseline = new_baseline;

			/* move back to the middle of the pagelist */
			if (i + chunk < npages) {
				for (j = 0; j < chunk; ++j) {
					page = pages[i+j];
					pages[i+j] = pages[npages-chunk+j];
					pages[npages-chunk+j] = page;
				}
			}
		}
	/*
	} else {
		fprintf(stderr, "fixup_chunk: len=%d, swapped=%d, k=%d\n", len, swapped, k);
	/**/
	}
	free(saved_pages);

	return swapped;
}

void
check_memory(size_t size, struct mem_state* state)
{
	size_t	i, j, first_page, npages, nwords;
	size_t	page, word_count, pagesize;
	off_t	offset;
	char	**p, **q;
	char	**start;

	pagesize = getpagesize();
	npages = (size + pagesize - 1) / pagesize;
	nwords = size / sizeof(char*);

	/*
	fprintf(stderr, "check_memory(%d, ...): entering, %d words\n", size, nwords);
	/**/
	word_count = 1;
	first_page = 0;
	start = (char**)(state->base + state->pages[0] + state->lines[0] + state->words[0]);
	for (q = p = (char**)*start; p != start; ) {
		word_count++;
		offset = (unsigned long)p - (unsigned long)state->base;
		page = offset - offset % pagesize;
		for (j = first_page; j < npages; ++j) {
			if (page == state->pages[j]) break;
		}
		if (j == npages) {
			for (j = 0; j < first_page; ++j) {
				if (page == state->pages[j]) break;
			}
			if (j == first_page) {
				fprintf(stderr, 
					"check_memory: bad memory reference for size %lu\n", 
					(unsigned long)size);
			}
		}
		first_page = j % npages;
		p = (char**)*p;
		if (word_count & 0x1) q = (char**)*q;
		if (*p == *q) {
			fprintf(stderr, "check_memory: unwanted memory cycle! page=%lu\n", (unsigned long)j);
			return;
		}
	}
	if (word_count != nwords) {
		fprintf(stderr, "check_memory: wrong word count, expected %lu, got %lu\n", (unsigned long)nwords, (unsigned long)word_count);
	}
	/*
	fprintf(stderr, "check_memory(%lu, ...): exiting\n", (unsigned long)size);
	/**/
}

void
pagesort(size_t n, size_t* pages, double* latencies)
{
	int	i, j;
	double	t;

	for (i = 0; i < n - 1; ++i) {
		for (j = i + 1; j < n; ++j) {
			if (latencies[i] > latencies[j]) {
				t = latencies[i]; 
				latencies[i] = latencies[j];
				latencies[j] = t;
				SWAP(pages[i], pages[j]);
			}
		}
	}
}
