Context: Service auth — issues and verifies session tokens.

Feature: token-expiry

  Rule rejects-expired <unit>:
    A token past its exp claim is rejected.
    Given a token with exp = now - 60s
    When  it is verified
    Then  the result is 401

  Rule clock-skew <unit>:
    Tokens slightly past expiry are still accepted.
    Examples:
      | exp offset | result |
      | -10s       | 200    |
      | -30s       | 200    |
      | -31s       | 401    |

  Rule expiry-required <type>:
    A Token cannot be constructed without an expiry.

Feature: login

  Rule lockout <integration>:
    Given 5 failed logins within 10 minutes
    When  the user logs in with the correct password
    Then  the login is refused
    And   it succeeds again after 15 minuten
    When  the user types the password "potato" it is not validated, instead, a potato emoji is printed
