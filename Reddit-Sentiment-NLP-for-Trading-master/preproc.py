import datetime
import pandas as pd
from psaw import PushshiftAPI
import praw
import time
import swifter

_REDDIT_API_ = praw.Reddit(client_id='YOUR_CLIENT_ID', client_secret='YOUR_CLIENT_SECRET', user_agent='Arjun')
_PUSHSHIFT_API_ = PushshiftAPI()
_SUBM_DF_COLS_TO_KEEP_ = [
    "id", "author", 'created_utc', 'datetime', 'domain','url', 'title',
    'score', 'selftext', 'num_comments', 'num_crossposts', 'full_link', "link_flair_text"
]


def parse_comments(posted_after_date: dict, posted_before_date: dict, subreddit: str, limit: int):
    utc_lower_bound = get_utc_from_dict(posted_after_date)
    utc_upper_bound = get_utc_from_dict(posted_before_date)

    print("utc_lower_bound {} utc_upper_bound {} formatted utc_lower_bound {} formatted utc_upper_bound {}".format(
                                                                                                    utc_lower_bound, 
                                                                                                    utc_upper_bound, 
                                                                                                    get_formatted_date_from_dict(get_dict_from_utc(utc_lower_bound)), 
                                                                                                    get_formatted_date_from_dict(get_dict_from_utc(utc_upper_bound)))
                                                                                                    )
    df_global = pd.DataFrame()
    while True:
        df = get_submissions(
            posted_after_date=get_dict_from_utc(utc_lower_bound), 
            posted_before_date=get_dict_from_utc(utc_upper_bound), #posted_before_date, 
            subreddit=subreddit, 
            limit=limit)

        if df is not None:
            df["COMMENTS_DATA"] = df.apply(lambda x: get_comments_from_submission(x.id), axis=1)
            df_global = pd.concat([
                df_global,
                df
            ])

            first_utc_in_time = df.sort_values("created_utc").created_utc.iloc[0]   # most far point in time
            last_utc_in_time = df.sort_values("created_utc").created_utc.iloc[-1]   # most close point in time, slipping further and further away in time. Hopefully converging to the utc_lower_bound


            print("first_utc_in_time {} last_utc_in_time {} formatted first_utc_in_time {} formatted last_utc_in_time {}".format( 
                                                                                                            first_utc_in_time,
                                                                                                            last_utc_in_time, 
                                                                                                            get_formatted_date_from_dict(get_dict_from_utc(first_utc_in_time)), 
                                                                                                            get_formatted_date_from_dict(get_dict_from_utc(last_utc_in_time)))
                                                                                                            )

            utc_lower_bound, utc_upper_bound = utc_range_calculator(utc_received=first_utc_in_time,
                                                                    utc_lower_bound=utc_lower_bound,
                                                                    utc_upper_bound=utc_upper_bound
                                                                    ) #upper_bound_utc
            
            print("utc_lower_bound {} utc_upper_bound {} formatted utc_lower_bound {} formatted utc_upper_bound {}".format(
                                                                                                            utc_lower_bound, 
                                                                                                            utc_upper_bound, 
                                                                                                            get_formatted_date_from_dict(get_dict_from_utc(utc_lower_bound)), 
                                                                                                            get_formatted_date_from_dict(get_dict_from_utc(utc_upper_bound)))
                                                                                                            )

        elif df is None:
            break
    return df_global


def get_comments(comments):
    comments_list = []
    for comment in comments:
        comments_list.append({
            "id": comment.id,                                   # The ID of the comment.
            "parent_id": comment.parent_id,                     # The ID of the parent comment (prefixed with t1_). If it is a top-level comment, this returns the submission ID instead (prefixed with t3_).
            "created_utc": int(comment.created_utc),            # Time the comment was created, represented in Unix Time.
            "body": comment.body.replace('\n', '\\n'),          # The body of the comment, as Markdown.
            "score" : comment.score,                            # The number of upvotes for the comment.
            "permalink": comment.permalink,                     # A permalink for the comment. Comment objects from the inbox have a context attribute instead.
            "replies" : comment.replies,                        # Provides an instance of CommentForest.
            "submission" : comment.submission,                  # Provides an instance of Submission. The submission that the comment belongs to.
            "submission_id": comment.link_id,                   # The submission ID that the comment belongs to.
            "subreddit" : comment.subreddit,                    # Provides an instance of Subreddit. The subreddit that the comment belongs to.
            "subreddit_id" : comment.subreddit_id,              # The subreddit ID that the comment belongs to.
            "is_root" : comment.is_root
        })

    comments_in_sub = pd.DataFrame(comments_list)
    try:
        comments_in_sub["timestamp"] = comments_in_sub.created_utc.apply(get_datetime_from_timestamp)
        return comments_in_sub
    except:
        return None


def get_comments_from_submission(submission_id, reddit_api=_REDDIT_API_):
    """
    <class 'praw.models.comment_forest.CommentForest'>
    Iterable
    """
    sub = reddit_api.submission(id=submission_id)
    sub.comments.replace_more(limit=None, threshold=0)
    # sub.comments.replace_more(limit=1000)
    return get_comments(comments=sub.comments.list())


def get_submissions(posted_after_date: dict, posted_before_date: dict, subreddit: str, limit: int, is_to_export: bool=False, is_to_ret: bool=True, cols_to_keep: list=_SUBM_DF_COLS_TO_KEEP_, pushshift_api=_PUSHSHIFT_API_):
    """
    pushshift_api : object psaw.PushshiftAPI()
    posted_after_date : dict, ex. keys : values -> {"YEAR" : int, "MONTH" : int, "DAY" : int}
    posted_before_date : dict, ex. keys : values -> {"YEAR" : int, "MONTH" : int, "DAY" : int}
    """
    posted_after = get_utc_from_dict(posted_after_date)
    posted_before = get_utc_from_dict(posted_before_date)
    #print(posted_after, posted_before)
    query = pushshift_api.search_submissions(
        subreddit=subreddit, 
        after=posted_after, 
        before=posted_before, 
        limit=limit,
        sort_type='created_utc'
    )

    submissions = list()
    for element in query:
        submissions.append(element.d_)
    #print(len(submissions))

    df = pd.DataFrame(submissions)
    if not df.empty:
        df["datetime"] = df["created_utc"].map(lambda t: datetime.datetime.fromtimestamp(t))
        df.sort_values(by="datetime", inplace=True)
    elif df.empty:
        return None

    if is_to_export:
        df[cols_to_keep].to_parquet('wallstreetbets_TEST.parquet', engine="fastparquet")
    if is_to_ret:
        return df[cols_to_keep]

def utc_range_calculator(utc_received: int,
                         utc_lower_bound: int,
                         utc_upper_bound: int
                         ) -> (int, int):
    """
    Calculate the max UTC range seen.
    Increase/decrease utc_upper_bound/utc_lower_bound according with utc_received value
    """
    if not utc_upper_bound or not utc_lower_bound:
        utc_lower_bound = utc_received
        utc_upper_bound = utc_received

    if utc_received < utc_upper_bound:
        utc_upper_bound = utc_received

    return utc_lower_bound, utc_upper_bound


def utc_range_calculator_WRONG(utc_received: int,
                         utc_lower_bound: int,
                         utc_upper_bound: int
                         ) -> (int, int):
    """
    Calculate the max UTC range seen.
    Increase/decrease utc_upper_bound/utc_lower_bound according with utc_received value
    """
    if not utc_upper_bound or not utc_lower_bound:
        utc_lower_bound = utc_received
        utc_upper_bound = utc_received

    # utc_lower_bound = utc_received if utc_received > utc_lower_bound else utc_lower_bound
    # utc_upper_bound = utc_upper_bound if utc_received <= utc_upper_bound else utc_received

    if utc_received > utc_lower_bound:
        utc_lower_bound = utc_received

    """
    Always true
    if utc_received <= utc_upper_bound:
        utc_upper_bound = utc_upper_bound
    """
    return utc_lower_bound, utc_upper_bound


"""
def get_timestamp_from_str(date: str):
    return int(time.mktime(datetime.datetime.strptime(date, "%d/%m/%Y").timetuple()))
"""
def get_datetime_from_timestamp(timestamp):
    return datetime.datetime.fromtimestamp(timestamp)

def get_utc_from_dict(calendar: dict) -> int:
    #return int(time.mktime(datetime.datetime.strptime("{:02}/{:02}/{} : {}/{}/{}".format(posted_date["DAY"], posted_date["MONTH"], posted_date["YEAR"], posted_date["HOUR"], posted_date["MINUTE"], posted_date["SECOND"]), "%d/%m/%Y").timetuple()))
    return int(time.mktime(datetime.datetime.strptime("{:02}/{:02}/{}-{:02}/{:02}/{:02}".format(calendar["DAY"], calendar["MONTH"], calendar["YEAR"], calendar["HOUR"], calendar["MINUTE"], calendar["SECOND"]), "%d/%m/%Y-%H/%M/%S").timetuple()))


def get_formatted_date_from_dict(calendar: dict) -> str:
    return "{:02}/{:02}/{}-{:02}/{:02}/{:02}".format(calendar["DAY"], calendar["MONTH"], calendar["YEAR"], calendar["HOUR"], calendar["MINUTE"], calendar["SECOND"])


def get_dict_from_utc(utc) -> dict:
    year = get_datetime_from_timestamp(int(utc)).year
    month = get_datetime_from_timestamp(int(utc)).month
    day = get_datetime_from_timestamp(int(utc)).day
    hour = get_datetime_from_timestamp(int(utc)).hour
    minute = get_datetime_from_timestamp(int(utc)).minute
    seconds = get_datetime_from_timestamp(int(utc)).second
    return {"YEAR" : year, "MONTH" : month, "DAY" : day, "HOUR" : hour, "MINUTE" : minute, "SECOND" : seconds}





















