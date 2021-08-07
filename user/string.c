#include "user/user.h"
#include <limits.h>
#include "kernel/param.h"

typedef unsigned long uintptr_t;

#define ISDIGIT(c) (c >= '0' && c <= '9')
#define ISSPACE(c) (c == ' ' || c == '\t')
#define ISALPHA(c) ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'))
#define ISUPPER(c) (c >= 'A' && c <= 'Z')

/*
 * Convert a string to a long integer.
 *
 * Ignores `locale' stuff.  Assumes that the upper and lower case
 * alphabets and digits are each contiguous.
 */
long strtol(nptr, endptr, base) const char* nptr;
char** endptr;
int base;
{
	const char* s = nptr;
	unsigned long acc;
	int c;
	unsigned long cutoff;
	int neg = 0;
	int any;
	int cutlim;

	// check base
	if(base < 0 || base > 36)
	{
		// errno = EINVAL

		if(endptr)
		{
			*endptr = (char*)(uintptr_t)nptr;
		}

		return 0;
	}

	/*
	 * Skip white space and pick up leading +/- sign if any.
	 * If base is 0, allow 0x for hex and 0 for octal, else
	 * assume decimal; if base is already 16, allow 0x.
	 */
	do
	{
		c = *s++;
	} while(ISSPACE(c));

	if(c == '-')
	{
		neg = 1;
		c = *s++;
	}
	else if(c == '+')
	{
		c = *s++;
	}

	if((base == 0 || base == 16) && c == '0' && (*s == 'x' || *s == 'X'))
	{
		c = s[1];
		s += 2;
		base = 16;
	}
	else if((base == 0 || base == 2) && c == '0' && (*s == 'b' || *s == 'B'))
	{
		c = s[1];
		s += 2;
		base = 2;
	}

	if(base == 0)
	{
		base = c == '0' ? 8 : 10;
	}

	/*
	 * Compute the cutoff value between legal numbers and illegal
	 * numbers.  That is the largest legal value, divided by the
	 * base.  An input number that is greater than this value, if
	 * followed by a legal input character, is too big.  One that
	 * is equal to this value may be valid or not; the limit
	 * between valid and invalid numbers is then based on the last
	 * digit.  For instance, if the range for longs is
	 * [-2147483648..2147483647] and the input base is 10,
	 * cutoff will be set to 214748364 and cutlim to either
	 * 7 (neg==0) or 8 (neg==1), meaning that if we have accumulated
	 * a value > 214748364, or equal but the next digit is > 7 (or 8),
	 * the number is too big, and we will return a range error.
	 *
	 * Set any if any `digits' consumed; make it negative to indicate
	 * overflow.
	 */
	cutoff = neg ? -(unsigned long)LONG_MIN : LONG_MAX;
	cutlim = (int)(cutoff % (unsigned long)base);
	cutoff /= (unsigned long)base;

	for(acc = 0, any = 0;; c = *s++)
	{
		if(ISDIGIT(c))
		{
			c -= '0';
		}
		else if(ISALPHA(c))
		{
			c -= ISUPPER(c) ? 'A' - 10 : 'a' - 10;
		}
		else
		{
			break;
		}

		if(c >= base)
		{
			break;
		}

		if(any < 0 || acc > cutoff || (acc == cutoff && c > cutlim))
		{
			any = -1;
		}
		else
		{
			any = 1;
			acc *= (unsigned long)base;
			acc += (unsigned long)c;
		}
	}

	if(any < 0)
	{
		acc = neg ? (unsigned long)LONG_MIN : (unsigned long)LONG_MAX;
		//		errno = ERANGE;
	}
	else if(neg)
	{
		acc = -acc;
	}

	if(endptr != 0)
	{
		*endptr = (char*)(uintptr_t)(any ? s - 1 : nptr);
	}
	return (long)(acc);
}

double
strtod (const char *str, char **ptr)
{
  char *p;
  if (ptr == (char **)0)
    return atof (str);
  
  p = (char*) str;
  
  while (ISSPACE (*p))
    ++p;
  
  if (*p == '+' || *p == '-')
    ++p;
  /* INF or INFINITY.  */
  if ((p[0] == 'i' || p[0] == 'I')
      && (p[1] == 'n' || p[1] == 'N')
      && (p[2] == 'f' || p[2] == 'F'))
    {
      if ((p[3] == 'i' || p[3] == 'I')
          && (p[4] == 'n' || p[4] == 'N')
          && (p[5] == 'i' || p[5] == 'I')
          && (p[6] == 't' || p[6] == 'T')
          && (p[7] == 'y' || p[7] == 'Y'))
        {
          *ptr = p + 8;
          return atof (str);
        }
      else
        {
          *ptr = p + 3;
          return atof (str);
        }
    }
  /* NAN or NAN(foo).  */
  if ((p[0] == 'n' || p[0] == 'N')
      && (p[1] == 'a' || p[1] == 'A')
      && (p[2] == 'n' || p[2] == 'N'))
    {
      p += 3;
      if (*p == '(')
        {
          ++p;
          while (*p != '\0' && *p != ')')
            ++p;
          if (*p == ')')
            ++p;
        }
      *ptr = p;
      return atof (str);
    }
  /* digits, with 0 or 1 periods in it.  */
  if (ISDIGIT (*p) || *p == '.')
    {
      int got_dot = 0;
      while (ISDIGIT (*p) || (!got_dot && *p == '.'))
        {
          if (*p == '.')
            got_dot = 1;
          ++p;
        }
      /* Exponent.  */
      if (*p == 'e' || *p == 'E')
        {
          int i;
          i = 1;
          if (p[i] == '+' || p[i] == '-')
            ++i;
          if (ISDIGIT (p[i]))
            {
              while (ISDIGIT (p[i]))
                ++i;
              *ptr = p + i;
              return atof (str);
            }
        }
      *ptr = p;
      return atof (str);
    }
  /* Didn't find any digits.  Doesn't look like a number.  */
  *ptr = (char*)str;
  return 0.0;
}

char *strdup(const char *s) {
  char *str;
  char *p;
  int len = 0;

  while (s[len])
      len++;
  str = malloc(len + 1);
  p = str;
  while (*s)
      *p++ = *s++;
  *p = '\0';
  return str;
}

static int tmp_cnt = 0;

char *tempnam(const char *dir, const char *pfx) {
  char buf[MAXPATH];
  sprintf(buf, "./tmpfile%d", tmp_cnt++);
  return strdup(buf);
}

// TODO
char*
getenv(const char *varname)
{
  if(strcmp(varname, "ENOUGH")) {
    return strdup("1000000");
  }
  if (strcmp(varname, "TIMING_O")) {
    return strdup("0");
  }
  if (strcmp(varname, "LOOP_O")) {
    return strdup("0");
  }
  return 0;
}
