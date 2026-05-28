import degiroapi
from degiroapi.product import Product
import numpy as np
import pandas as pd
import datetime
import matplotlib.pyplot as plt
import math
from keras.models import Sequential
from keras.layers import Dense, LSTM
from sklearn.preprocessing import MinMaxScaler
from sklearn.metrics import mean_squared_error
from pathlib import Path
from tensorflow import keras
import hashlib
from degiro.Timer import Timer
from degiro import logger
#import swifter

_SERIES_ROOT_PATH_ = Path("C:\\Users\\fontanesio\\Documents\\pythonScripts\\progetti\\personali\\degiro\\degiro\\file\\raw\\ss\\")
_SERIES_PATH_      = Path("C:\\Users\\fontanesio\\Documents\\pythonScripts\\progetti\\personali\\degiro\\degiro\\file\\raw\\ss\\series\\")

_ROOT_TMP_PATH_    = Path("C:\\Users\\fontanesio\\Documents\\pythonScripts\\progetti\\personali\\degiro\\degiro\\file\\raw\\tmp\\")
_SERIES_TMP_PATH_  = Path("C:\\Users\\fontanesio\\Documents\\pythonScripts\\progetti\\personali\\degiro\\degiro\\file\\raw\\tmp\\series\\")

_MODEL_STATS_PATH_ = Path("C:\\Users\\fontanesio\\Documents\\pythonScripts\\progetti\\personali\\degiro\\degiro\\file\\processed\\parquet\\")
_MODEL_PATH_       = Path("C:\\Users\\fontanesio\\Documents\\pythonScripts\\progetti\\personali\\degiro\\degiro\\file\\processed\\models\\")

def massive_updater(conn_degiro:degiroapi.DeGiro, degiro_index:dict) -> None:
    #from degiro.degiroUtils import _ROOT_TMP_PATH_

    for payload in degiro_index["stockCountries"]:
        country_name = get_country_name(payload["id"], degiro_index)
        if "indices" in payload.keys() and "exchanges" in payload.keys():
            payload["indices_exchanges"] = payload["indices"] + payload["exchanges"]
            target = "indices_exchanges"
        elif "indices" in payload.keys():
            target = "indices"
        elif "exchanges" in payload.keys():
            target = "exchanges"

        for index_id in payload[target]:
            exchange_name = get_index_name(index_id, degiro_index)

            logger.debug("------> TRY : {} - {} - {} - {}".format(payload["id"], country_name, index_id, exchange_name))
            try:

                exit_status = updater(
                    conn_degiro=conn_degiro, 
                    index_id=index_id, 
                    stock_country_id=payload["id"], 
                    export_path=_ROOT_TMP_PATH_,
                    time_span="1Y"
                )                

                if exit_status == 0:
                    logger.debug("------> DONE : {} - {} - {} - {}".format(payload["id"], country_name, index_id, exchange_name))
                elif exit_status == -1:
                    logger.debug("------> NO : {} - {} - {} - {}".format(payload["id"], country_name, index_id, exchange_name))
            except Exception as e:
                logger.debug("------> NO : {} - {} - {} - {}".format(payload["id"], country_name, index_id, exchange_name))
                logger.debug("\t{}".format(e))

    return None

def updater(
    conn_degiro: degiroapi.DeGiro, 
    index_id : int, 
    stock_country_id : int, 
    export_path: Path=_ROOT_TMP_PATH_,
    time_span: str="1Y"
    ) -> None:

    # scrivo le tmp in raw_file_path / tmp
    exit_status = downloader(
        conn_degiro=conn_degiro, 
        index_id=index_id, 
        stock_country_id=stock_country_id, 
        raw_file_path=export_path,
        time_span=time_span
        )

    if exit_status == -1:
        logger.debug("Skip {}-{} : downloader exit_stauts = {}".format(index_id, stock_country_id, exit_status))
        return exit_status

    ts_new = pd.read_parquet(_SERIES_TMP_PATH_ / f"{index_id}_{stock_country_id}_ts.parquet", engine="fastparquet")
    ts_old = pd.read_parquet(_SERIES_PATH_     / f"{index_id}_{stock_country_id}_ts.parquet", engine="fastparquet")

    ts_up_to_date = pd.concat([ts_old, ts_new])
    ts_up_to_date = ts_up_to_date[~ts_up_to_date.sort_index().index.duplicated(keep='first')]
    ts_up_to_date.to_parquet(_SERIES_PATH_ / f"{index_id}_{stock_country_id}_ts.parquet", engine="fastparquet")

    logger.debug("Dim old > rows : {} - cols : {}".format(ts_old.shape[0], ts_old.shape[1]))
    logger.debug("Dim new > rows : {} - cols : {}".format(ts_up_to_date.shape[0], ts_up_to_date.shape[1]))
    if ts_old.shape[0] > 0:
        logger.debug("Date old > last obs : {} - first obs : {}".format(ts_old.index[0], ts_old.index[-1]))
    elif ts_old.shape[0] == 0:
        logger.debug("Date old > last obs : {} - first obs : {}".format("None", "None"))
    elif ts_up_to_date.shape[0] > 0:
        logger.debug("Date new > last obs : {} - first obs : {}".format(ts_up_to_date.index[0], ts_up_to_date.index[-1]))
    elif ts_up_to_date.shape[0] == 0:
        logger.debug("Date new > last obs : {} - first obs : {}".format("None", "None"))

    return exit_status
    
def massive_downloader(conn_degiro:degiroapi.DeGiro, degiro_index:dict) -> None:
    for payload in degiro_index["stockCountries"]:
        country_name = get_country_name(payload["id"], degiro_index)
        if "indices" in payload.keys() and "exchanges" in payload.keys():
            payload["indices_exchanges"] = payload["indices"] + payload["exchanges"]
            target = "indices_exchanges"
        elif "indices" in payload.keys():
            target = "indices"
        elif "exchanges" in payload.keys():
            target = "exchanges"

        for index_id in payload[target]:
            exchange_name = get_index_name(index_id, degiro_index)

            logger.debug("------> TRY : {} - {} - {} - {}".format(payload["id"], country_name, index_id, exchange_name))
            try:
                exit_status = downloader(
                    conn_degiro=conn_degiro, 
                    index_id=index_id, 
                    stock_country_id=payload["id"], 
                    raw_file_path=_SERIES_ROOT_PATH_,
                    time_span="1Y"
                    )
                
                if exit_status == 0:
                    logger.debug("------> DONE : {} - {} - {} - {}".format(payload["id"], country_name, index_id, exchange_name))
                elif exit_status == -1:
                    logger.debug("------> NO : {} - {} - {} - {}".format(payload["id"], country_name, index_id, exchange_name))
            except Exception as e:
                logger.debug("------> NO : {} - {} - {} - {} - {}".format(e, payload["id"], country_name, index_id, exchange_name))

    return None

def get_country_name(country_id:int, payloads:dict):
    for country in payloads["countries"]:
        if country["id"] == country_id:
            return country["name"]
    return None

@DeprecationWarning
def get_index_name_old(country_id:int, index_id:int, payloads:dict):
    for country in payloads["eurexCountries"]:
        if country["id"] == country_id:
            for exchange in country["exchanges"]:
                if exchange["id"] == index_id:
                    return (exchange["code"], exchange["country"], exchange["name"])
    return (None, None, None)

def get_index_name(index_id:int, payloads:dict):
    for index in payloads["indices"]:
        if index["id"] == index_id:
            return index["name"]
    for index in payloads["exchanges"]:
        if index["id"] == index_id:
            return index["name"]
    return None

def downloader(
    conn_degiro: degiroapi.DeGiro, 
    index_id : int, 
    stock_country_id : int, 
    raw_file_path: Path,
    time_span: str="1Y"
    ):
    df = download_stock_info(
        degiro=conn_degiro, 
        index_id=index_id, 
        stock_country_id=stock_country_id, 
        time_span=time_span
        )
    if df.shape[0] == 0:
        return -1
    
    df["ISIN"] = df["ANAGRAFICA"].apply(lambda x: x["isin"])
    df_anagrafica = anagrafica_builder(df, "ANAGRAFICA")
    df_anagrafica.set_index("isin", inplace=True)

    df["TIME_SERIES"] = df.apply(parse_hist_data, axis=1)
    ts_df = time_series_builder(df)
    ts_df.sort_values("DATE", ascending=True, inplace=True)

    # to_parquet NON esporta colonne in formato lista
    """
    try:
        dax_anagrafica.to_parquet(raw_file_path / f"{index_id}_{stock_country_id}_anagrafica.parquet", engine="fastparquet")
    except:
        dax_anagrafica.to_csv(raw_file_path / f"{index_id}_{stock_country_id}_anagrafica.csv")

    to_drop = dax_anagrafica.iloc[0].apply(lambda x : isinstance(x, list)).values
    dax_anagrafica.drop(columns=dax_anagrafica.columns[to_drop]).to_parquet(raw_file_path / f"{index_id}_{stock_country_id}_anagrafica.parquet")
    """
    
    df_anagrafica.to_parquet(raw_file_path / "anagrafica" / f"{index_id}_{stock_country_id}_anagrafica.parquet")
    ts_df.to_parquet(raw_file_path / "series" / f"{index_id}_{stock_country_id}_ts.parquet", engine="fastparquet")
    return 0

def download_stock_info(
    degiro:degiroapi.DeGiro, 
    index_id: int, 
    stock_country_id: int, 
    time_span: str="1Y"
    ):
    """
    get_stock_list(self, indexId, stockCountryId)
        s&p 500 stock list : 14, 846
        german30 stock list : 6, 906
        time_span : [1D, 1W, 1Y] -> degiroapi.Interval.Type.One_Day, degiroapi.Interval.Type.One_Week, degiroapi.Interval.Type.One_Year
        Interval can be set to One_Day, One_Week, One_Month, Three_Months, Six_Months, One_Year, Three_Years, Five_Years, Max
    """
    prod_list = []
    scarti_list = []
    try:
        products = degiro.get_stock_list(index_id, stock_country_id)
    except Exception as e:
        logger.debug(e)
        return pd.DataFrame(data=None, columns=["P_REAL_TIME", "P_HISTORICAL", "ANAGRAFICA"])

    for product in products:
        prod_list.append(Product(product))

    df_prod = {}
    for i, prod in enumerate(prod_list):
        try:
            prod_info = degiro.product_info(prod.id)
            if time_span == "1D":
                price_info = degiro.real_time_price(prod.id, degiroapi.Interval.Type.One_Day)
            elif time_span == "1W":
                price_info = degiro.real_time_price(prod.id, degiroapi.Interval.Type.One_Week)
            elif time_span == "1Y":
                price_info = degiro.real_time_price(prod.id, degiroapi.Interval.Type.One_Year)
            price_info.append(prod_info)
            df_prod[prod.id] = price_info
        except Exception as e:
            scarti_list.append(prod)
            logger.debug(e)

        logger.debug(f"{i + 1} : {len(prod_list)} - {prod.id}")
        #print(f"\r{i}", end="")
    
    logger.debug(f"Failed stocks {len(scarti_list)}")
    logger.debug(f"Success stocks {len(df_prod)}")
    return pd.DataFrame(df_prod).T.rename(columns={0 : "P_REAL_TIME", 1 : "P_HISTORICAL", 2 : "ANAGRAFICA"})

def add_return(price_vector: list):
    return [np.log(pd.Series(price_vector)/pd.Series(price_vector).shift(1)).to_list()[1:]][0]

def add_average(vector: list, lag: int):
    return np.mean(vector[lag:])

def add_std_dev(vector: list, lag: int):
    return np.std(vector[lag:])

def plot_time_interval(stock_id, df, field, from_date=None, to_date=None):
    ax = pd.Series(df.loc[stock_id, field][from_date:to_date]).plot(lw=1, colormap='jet', marker='.', markersize=0.1, title=df.loc[stock_id, "ISIN"])
    ax.set_xlabel("Delta t")
    ax.set_ylabel("S")

def set_threshold(r: float, lower_boundary=0.05, upper_boundary=0.05):
    if r < lower_boundary:
        return "SELL"
    elif r > upper_boundary:
        return "BUY"
    else:
        return "HOLD AND PRAY"

def set_threshold_2(v_r: float, lower_boundary=-0.025, upper_boundary=0.025):
    v_t = []
    for r in v_r:
        if r < lower_boundary:
            v_t.append("SELL")
        elif r > upper_boundary:
            v_t.append("BUY")
        else:
            v_t.append("HOLD AND PRAY")
    return v_t

def set_threshold_3(v_r: float, lower_boundary=-0.025, upper_boundary=0.025):
    v_t = []
    for r in v_r:
        if r < lower_boundary:
            v_t.append(-1)
        elif r > upper_boundary:
            v_t.append(1)
        else:
            v_t.append(0)
    return v_t

def parse_hist_data(row):    
    hist_info = row.P_HISTORICAL
    isin = row.ISIN

    days = [obs[0] for obs in hist_info["data"]]
    from_dt = hist_info["times"].split("/")[0]
    to_dt   = hist_info["expires"].split("T")[0]
    from_dt = datetime.datetime.strptime(from_dt, '%Y-%m-%d')
    to_dt   = datetime.datetime.strptime(to_dt, '%Y-%m-%d')
    logger.debug("from : {} to : {}".format(from_dt, to_dt))
    date_list = [to_dt - datetime.timedelta(days=x) for x in days]
    tt = pd.concat([
                pd.Series(date_list),
                pd.Series([obs[1] for obs in hist_info["data"]])
                ], axis=1).rename(columns={0 : "DATE", 1 : isin}).set_index("DATE")
    return tt

def time_series_builder(df):
    ts_df = pd.DataFrame(index=df.iloc[0].TIME_SERIES.index, columns=["--"], data=-100)
    for _, row in df.iterrows():
        ts_df = ts_df.merge(row["TIME_SERIES"], how="inner", left_index=True, right_index=True)
    ts_df.drop(columns=["--"], inplace=True)
    return ts_df

def create_dataset(dataset, look_back=1):
	dataX, dataY = [], []
    # len(dataset) - (look_back-1)
	for i in range(len(dataset) - look_back):
		dataX.append(dataset[i:(i+look_back), 0])
		dataY.append(dataset[i + look_back, 0])
	return np.array(dataX), np.array(dataY)

def rnn_train(
    dataset, 
    look_back=1, 
    epochs=100, 
    loss="mean_squared_error", 
    optimizer="adam", 
    verbose=2, 
    in_sample_perc=0.675, 
    lstm_dim=4, 
    dense_dim=1, 
    batch_size=1
    ):
    np.random.seed(7)
    dataset = pd.DataFrame(dataset).values

    # normalize the dataset
    scaler = MinMaxScaler(feature_range=(0, 1))
    dataset = scaler.fit_transform(dataset)
    # split into train and test sets
    train_size = int(len(dataset) * in_sample_perc)
    # test_size = len(dataset) - train_size
    train, test = dataset[0:train_size,:], dataset[train_size:len(dataset),:]

    # reshape into X=t and Y=t+1
    trainX, trainY = create_dataset(train, look_back)
    # testX, testY = create_dataset(test, look_back)
    # reshape input to be [samples, time steps, features]
    trainX = np.reshape(trainX, (trainX.shape[0], 1, trainX.shape[1]))
    #testX = np.reshape(testX, (testX.shape[0], 1, testX.shape[1]))

    # create and fit the LSTM network
    model = Sequential()
    model.add(LSTM(lstm_dim, input_shape=(1, look_back)))
    model.add(Dense(dense_dim))
    model.compile(loss=loss, optimizer=optimizer)
    model.fit(trainX, trainY, epochs=epochs, batch_size=batch_size, verbose=verbose)

    return model, scaler.data_min_[0], scaler.data_max_[0], None, None, None, None

def rnn_predict_dataset(model, dataset, scaler_min, scaler_max, look_back=2, in_sample_perc=0.67):
    np.random.seed(7)
    dataset = pd.DataFrame(dataset).values

    dataset = (dataset - scaler_min) / (scaler_max - scaler_min)
    train_size = int(len(dataset) * in_sample_perc)
    test_size = len(dataset) - train_size
    train, test = dataset[0:train_size,:], dataset[train_size:len(dataset),:]
    testX, testY = create_dataset(test, look_back)
    testX = np.reshape(testX, (testX.shape[0], 1, testX.shape[1]))

    testPredict = model.predict(testX)
    testPredict = testPredict * (scaler_max - scaler_min) + scaler_min
    testY = testY * (scaler_max - scaler_min) + scaler_min

    return testPredict, testY

def rnn_get_stats_2(model, dataset, look_back=1, in_sample_perc=0.67):
    np.random.seed(7)
    dataset = pd.DataFrame(dataset).values

    # normalize the dataset
    scaler = MinMaxScaler(feature_range=(0, 1))
    dataset = scaler.fit_transform(dataset)
    # split into train and test sets
    train_size = int(len(dataset) * in_sample_perc)
    test_size = len(dataset) - train_size
    train, test = dataset[0:train_size,:], dataset[train_size:len(dataset),:]

    # reshape into X=t and Y=t+1
    trainX, trainY = create_dataset(train, look_back)
    testX, testY = create_dataset(test, look_back)
    # reshape input to be [samples, time steps, features]
    trainX = np.reshape(trainX, (trainX.shape[0], 1, trainX.shape[1]))
    testX = np.reshape(testX, (testX.shape[0], 1, testX.shape[1]))

    trainPredict = model.predict(trainX)
    testPredict = model.predict(testX)

    # invert predictions
    trainPredict = scaler.inverse_transform(trainPredict)
    trainY = scaler.inverse_transform([trainY])

    testPredict = scaler.inverse_transform(testPredict)
    testY = scaler.inverse_transform([testY])

    # calculate root mean squared error
    trainScore = math.sqrt(mean_squared_error(trainY[0], trainPredict[:,0]))
    testScore = math.sqrt(mean_squared_error(testY[0], testPredict[:,0]))

    error_volatility_in_sample = np.std(trainY[0] - trainPredict[:,0])
    error_volatility_out_of_sample = np.std(testY[0] - testPredict[:,0])

    return trainScore, testScore, error_volatility_in_sample, error_volatility_out_of_sample

def get_id(isin):
    id_str = isin # str(datetime.datetime.now().timestamp()) + 
    return id_str.encode("utf-8").hex()

def rnn_save(model, path):
    """
    https://stackoverflow.com/questions/65697623/tensorflow-warning-found-untraced-functions-such-as-lstm-cell-6-layer-call-and
    added save_format="h5" inorder to avoid exceptions
    """
    model.save(path, save_format="h5")

def rnn_load(path):
    return keras.models.load_model(path, compile=False)

def plot_oos(df, isin):
    fig = plt.figure(figsize=(20, 7))
    plt.grid()
    ax = fig.gca()
    ax.set_xticks(np.arange(0, 100, 1))
    ax.set_yticks(np.arange(0, 100, 1))

    # plt.plot(scaler.inverse_transform(dataset[-80:]))
    plt.plot(df.loc[isin].TEST_Y[:-1])
    plt.plot(df.loc[isin].PREDICTED_Y[1:])
    plt.show()

def sample_field_parser(s, look_back):
    return "SAMPLE_" + str( int( s.split("_")[1] ) - look_back)

def cut(df, level, key):
    return df.iloc[:, df.columns.get_level_values(level)==key]
def reshape_ts(ts: list):
    return np.reshape(ts, (ts.shape[0], 1, ts.shape[1]))

def get_log_ret(ground_true: pd.DataFrame, ground_prediction: pd.DataFrame, isin: list):
    return np.log(pd.concat([
        ground_true.loc[isin],
        ground_prediction.loc[isin]
    ]).astype(float).T.div(pd.concat([
        ground_true.loc[isin],
        ground_prediction.loc[isin]
    ]).astype(float).T.shift(1)))

def get_log_ret_gt_vs_pred(ground_true: pd.DataFrame, ground_prediction: pd.DataFrame, isin: list):
    return np.log(ground_prediction.loc[isin].astype(float).T.div(ground_true.loc[isin].astype(float).T.shift(1)))

def get_look_back_window(df, super_key, mid_key, lower_key):
    return df[(super_key, mid_key)].T[lower_key]

def min_max_scaler(ts: list, ts_min: float, ts_max: float):
    return (ts - ts_min)/(ts_max - ts_min)

"""
def min_max_scaler(ts: list, ts_min: float, ts_max: float):
    try:
        return (ts - ts_min)/(ts_max - ts_min)
    except Exception as e:
        logger.debug(e, ts, ts_min, ts_max) 
        return None
"""

def min_max_scaler_inverse(ts: list, ts_min: float, ts_max: float):
    return ts * (ts_max - ts_min) + ts_min

def reshape_ts(ts: list):
    return np.reshape(ts, (ts.shape[0], 1, ts.shape[1]))

def get_prediction(isin: str, sample, models_df, samples_df):
    input_sample = []
    ts_min = models_df.loc[isin].SCALER_MIN#.values[0]
    ts_max = models_df.loc[isin].SCALER_MAX#.values[0]
    input_sample.append(
        np.array(min_max_scaler(get_look_back_window(samples_df, "SAMPLES", sample, isin).values, ts_min, ts_max))
    )
    input_sample = reshape_ts(np.array(input_sample)).astype('float32')
    return min_max_scaler_inverse(models_df.loc[isin, "MODEL"].predict(input_sample), ts_min, ts_max)[0][0]

"""
def get_prediction(isin, sample, models_df, samples_df):
    input_sample = []
    ts_min = models_df.loc[isin].SCALER_MIN
    ts_max = models_df.loc[isin].SCALER_MAX
    input_sample.append(
        np.array(min_max_scaler(get_look_back_window(samples_df, "SAMPLES", sample, isin).values, ts_min, ts_max))
    )
    input_sample = reshape_ts(np.array(input_sample))
    return min_max_scaler_inverse(models_df.loc[isin, "MODEL"].predict(input_sample), ts_min, ts_max)[0][0]
"""

def sample_field_parser(s: str, look_back: int):
    return "SAMPLE_" + str( int(s.split("_")[1]) - look_back)

def anagrafica_builder(df : pd.DataFrame, anagrafica_field: str):
    cols = df[f"{anagrafica_field}"].iloc[0].keys()
    anagrafica = pd.DataFrame(index=df.index, columns=cols)
    for stock_id in df.index:
        anagrafica.loc[stock_id] = df[f"{anagrafica_field}"].loc[stock_id]
    return anagrafica

def check_equality_btwn_df(df_1, df_2):
    from tabulate import tabulate

    if df_2.equals(df_1):
        logger.debug("DFS ARE EQUALS")
        return None
    logger.debug("DFS DIFFERS")    
    for col in df_1.columns:
        if df_2[col].equals(df_1[col]):
            logger.debug(f"OK - EQUALS COL : {col}")
        else:
            logger.debug("\n\tKO - DIFF COL : {} ".format(col))
            logger.debug("\tdtypes : {} {}".format(df_1[col].dtype, df_2[col].dtype))
            logger.debug(tabulate(pd.DataFrame(df_1[col].describe()), headers="keys", tablefmt="psql"))
            logger.debug(tabulate(pd.DataFrame(df_2[col].describe()), headers="keys", tablefmt="psql"))

"""
def timer(start,end):
    hours, rem = divmod(end-start, 3600)
    minutes, seconds = divmod(rem, 60)
    return int(hours), int(minutes), int(seconds)

def timer_from_base_sec(rem):
    minutes, seconds = divmod(rem, 60)
    return int(minutes), int(seconds)

def get_timer_stats(start_time, exec_time_by_iteration):
    stop_time = time.time()
    hours, minutes, seconds = timer(start=start_time, end=stop_time)
    time_base_sec = stop_time - start_time
    exec_time_by_iteration.append(time_base_sec)
    residual_iterations = total_permutations - iteration
    logger.debug("--- {:0>2}:{:0>2}:{:0>2} ---> Execution time (min) : last iteration".format(int(hours),int(minutes),seconds))

    avg_exc_time_hours, avg_exc_time_minutes, avg_exc_time_seconds = timer(start=np.mean(exec_time_by_iteration), end=0)
    logger.debug("--- {:0>2}:{:0>2}:{:0>2} ---> Average execution time (min) for {} iterations".format(avg_exc_time_hours,avg_exc_time_minutes, avg_exc_time_seconds, iteration))

    exp_res_hours, exp_res_minutes, exp_res_seconds = timer(residual_iterations*np.mean(exec_time_by_iteration), 0)
    logger.debug("--- {:0>2}:{:0>2}:{:0>2} ---> Remaining estimated time (min) for {} iterations".format(exp_res_hours, exp_res_minutes, exp_res_seconds, residual_iterations))

    past_hours, past_minutes, past_seconds = timer(sum(exec_time_by_iteration), 0)
    logger.debug("--- {:0>2}:{:0>2}:{:0>2} ---> Total time (min) spent on {} iterations".format(past_hours, past_minutes, past_seconds, len(exec_time_by_iteration)))
"""

def from_dict_values_to_hash(dictionary: dict):
    try:
        list_to_hash = [str(int) for int in dictionary.values()]
        str_to_hash = "".join(list(list_to_hash))
        return hashlib.md5(str_to_hash.encode()).hexdigest()
    except:
        logger.debug(dictionary)

def build_dict_for_hashing(orig_param, new_param):
    orig_param.update(new_param)
    return orig_param

def get_param_permutation(grid_param):
    import itertools
    sorted_grid_param = sorted(grid_param)
    combinations = list(itertools.product(*(grid_param[Name] for Name in sorted_grid_param)))
    permutations = []
    for combination in combinations:
        tmp_param = {}
        for sorted_param, param in zip(sorted_grid_param, combination):
            tmp_param.update({sorted_param : param})
        permutations.append(tmp_param)
    return permutations

def train_models_on_df(
    ts : pd.DataFrame, 
    model_param: dict, 
    return_param_hash=False, 
    return_stats=False
    ):
    from tabulate import tabulate

    param_hash = from_dict_values_to_hash(model_param)
    logger.debug("Param ID...{}".format(param_hash))

    chunks_len = int(len(ts.columns)/model_param["chunks_num"])
    logger.debug("Isin per chunk...{}".format(chunks_len))

    rnn_models = pd.DataFrame(index=ts.columns, columns=["MODEL", "SCALER_MIN", "SCALER_MAX", "TRAIN_LOSS", "TEST_LOSS", "VOL_IS", "VOL_OOS", "MODEL_HASH"], data=None)
    # TODO: rnn_models.index.name = "ISIN"
    for chunk_num in range(model_param["chunks_num"]):
        logger.debug("Start trainig chunk...{}".format(chunk_num+1))
        columns = ts.columns[(chunk_num*chunks_len):(chunk_num*chunks_len+chunks_len)]
        logger.debug("Working on : ", list(columns))

        model_tmp = ts[columns].apply(rnn_train, epochs=model_param["epochs"], look_back=model_param["look_back"], batch_size=model_param["batch_size"], loss=model_param["loss"], optimizer=model_param["optimizer"], lstm_dim=model_param["lstm_dim"], dense_dim=model_param["dense_dim"], result_type="expand", verbose=0)

        model_tmp_t = model_tmp.T
        model_tmp_t["MODEL_HASH"] = None
        rnn_models.loc[columns] = pd.DataFrame(data=model_tmp_t).rename(columns={
            0 : "MODEL",
            1 : "SCALER_MIN",
            2 : "SCALER_MAX",
            3 : "TRAIN_LOSS",
            4 : "TEST_LOSS",
            5 : "VOL_IS",
            6 : "VOL_OOS"
        })

        rnn_models.loc[columns, "TIMESTAMP"] = datetime.datetime.now().timestamp()

        # Building model specific index (hashing isin, timestamp and param)
        rnn_models.loc[columns, "MODEL_HASH"] = (rnn_models.loc[columns].reset_index().rename(columns={"index" : "ISIN"}).apply(
            lambda x : from_dict_values_to_hash(build_dict_for_hashing(model_param, {"isin" : x.ISIN, "time" : x.TIMESTAMP})
            ), axis=1)).values

        logger.debug(f"Saving models...")
        rnn_models.loc[columns].apply(
            lambda x: rnn_save(x.MODEL, path=_MODEL_PATH_ / "{}".format(x.MODEL_HASH)), axis=1
            )
        logger.debug(f"Models saved...")

    logger.debug(f"Getting stats...")
    rnn_models[["TRAIN_LOSS", "TEST_LOSS", "VOL_IS", "VOL_OOS"]] = rnn_models.apply(
        lambda x: rnn_get_stats_2(model=x.MODEL, dataset=ts[x.name], look_back=model_param["look_back"]), result_type="expand", axis=1
        )
    rnn_models["AVERAGE_PRICE"] = ts.mean()
    rnn_models["LOOK_BACK"] = model_param["look_back"]
    rnn_models["LAST_PRICE"] = ts.iloc[0]
    rnn_models["TRAIN_LOSS_PERC"] = rnn_models["TRAIN_LOSS"].div(rnn_models["AVERAGE_PRICE"])
    rnn_models["TEST_LOSS_PERC"] = rnn_models["TEST_LOSS"].div(rnn_models["AVERAGE_PRICE"])
    rnn_models["VOL_IS_PERC"] = rnn_models["VOL_IS"].div(rnn_models["AVERAGE_PRICE"])
    rnn_models["VOL_OOS_PERC"] = rnn_models["VOL_OOS"].div(rnn_models["AVERAGE_PRICE"])
    rnn_models["PARAM_HASH"] = param_hash
    
    stats = rnn_models[["TRAIN_LOSS", "TEST_LOSS", "VOL_IS", "VOL_OOS"]].describe()
    logger.debug(tabulate(stats, headers='keys', tablefmt='psql'))

    logger.debug(f"Writing session to parquet...")
    rnn_models.reset_index().drop(columns=["MODEL"]).to_parquet(_MODEL_STATS_PATH_ / "models_{}.parquet".format(param_hash), engine="fastparquet")

    if return_param_hash and return_stats:
        return param_hash, stats
    elif return_param_hash:
        return param_hash
    elif return_stats:
        return stats
    else:
        pass

def train_on_grid(
    global_param, 
    grid_param, 
    ts, 
    return_param_hash=True, 
    return_stats=True
    ):
    from copy import deepcopy
    import time

    param_permutations = get_param_permutation(grid_param)
    total_permutations = len(param_permutations)

    param_df = pd.DataFrame()
    stats_df = pd.DataFrame()
    logger.debug("Total iterations...{}".format(total_permutations))
    
    timer = Timer(total_iterations=total_permutations)

    for iteration, model_p in enumerate(param_permutations):
        iteration += 1
        start_time = time.time()
        logger.debug(model_p)
        
        model_p.update(global_param)
        param_hash, stats = train_models_on_df(
            ts=ts, 
            model_param=deepcopy(model_p), 
            return_param_hash=return_param_hash, 
            return_stats=return_stats
            )

        stats["PARAM_HASH"] = param_hash
        stats.reset_index(inplace=True)
        stats.rename(columns={"index" : "STAT"}, inplace=True)
        stats.set_index(["PARAM_HASH", "STAT"], inplace=True)
        stats_df = pd.concat([stats_df, stats])

        param_tmp = pd.DataFrame(model_p, index=[param_hash])
        param_df = pd.concat([param_df, param_tmp])
        logger.debug("Done {} -> {}%".format(iteration, (iteration)/len(param_permutations)))

        timer.get_stats(start_time=start_time, iteration=iteration)

    param_df.index.name = "PARAM_HASH"
    return param_df, stats_df

def compute_log_ret(ts: pd.DataFrame, price_threshold:int=-1):
    """
    :param ts: dataframe con vettore prezzi in colonna ordinati per data decrescente 
    :param price_threshold: soglia di prezzo oltre la quale non considero la serie storica 
    """
    max_price = ts.apply(max)
    ts_cutted = ts[max_price[max_price < price_threshold].index]
    ts_cutted = ts_cutted.apply(lambda price: price.pct_change())
    return np.log1p(ts_cutted).drop(index=ts_cutted.index[0])



def build_predicted_df(models_df: pd.DataFrame, ts: pd.DataFrame, look_back: int):
    """
    TODO:
        Fct da wrappare passando come input :
            fare distinct valori look_back
            splittare models_df e ts (entrambi hanno index isin) per livello di look_back relativo a tali prodotti
            invocare build_predicted_df con look_back e relativo ritaglio di models_df e ts
    """
    from degiro.degiroUtils import sample_field_parser, get_prediction

    # Genero ss : columns -> columns di ss; ts.columns -> index di ss
    sample_col = [f"SAMPLE_{sample}" for sample in range(ts.shape[0]-look_back)]
    date_col = [(ts.index[positional:(positional+look_back)]) for positional, date in enumerate(ts.index[:-look_back])]

    columns = []
    for c, sample in enumerate(sample_col):
        for obs_date in date_col[c]:
            columns.append((sample, obs_date))

    ss = pd.DataFrame(index=ts.columns, columns=pd.MultiIndex.from_tuples(columns))
    last_date = ss.columns.get_level_values(level=1)[-2]
    for i, obs_date in enumerate(ts.index[:len(date_col)]):
        if obs_date >= last_date:
            logger.debug(obs_date, last_date)
            break
        try:
            date_window = ts.index[i:(i+look_back)]
            cols = ss.columns[ss.columns.get_level_values(level=0)==f"SAMPLE_{i}"]
            ss[cols] = ts.loc[date_window].T.astype(np.float)
            
        except:
            #logger.debug(i)
            #logger.debug(date_window)
            logger.debug(cols)

    ss_T = ss.T
    ss_T.index.set_names(["SAMPLE", "DATE"], inplace=True)
    ss_T["Y"] = "GROUND_TRUE"
    ss_T.reset_index("SAMPLE", inplace=True)
    ss_T.rename(columns={"SAMPLE" : "SAMPLE_OLD"}, inplace=True)
    ss_T["SAMPLE"] = ss_T.SAMPLE_OLD.apply(sample_field_parser, look_back=look_back)

    ground_truth = ss_T.reset_index().groupby("SAMPLE_OLD").head(1).iloc[look_back:-1].set_index(["Y", "SAMPLE", "DATE"]).T
    pp = ss.T.reset_index("DATE").iloc[:-look_back*(look_back+1)].set_index("DATE", append=True)
    pp["Y"] = "SAMPLES"
    pp = pp.reset_index().set_index(["Y", "SAMPLE", "DATE"])
    ss = pp.T
    ss = pd.merge(ss, ground_truth, how="inner", left_index=True, right_index=True)

    ground_prediction = pd.DataFrame(index=ss[("GROUND_TRUE")].index, columns=ss[("GROUND_TRUE")].columns, data=None)
    tot_stocks=len(ground_prediction.index)
    for c, isin in enumerate(ground_prediction.index):
        for sample, data in zip(ground_prediction.columns.get_level_values(0), ground_prediction.columns.get_level_values(1)):
            ground_prediction.loc[isin, (sample, data)] = get_prediction(isin, sample, models_df, ss)
        logger.debug(round(c/tot_stocks, 4))

    samples = ss[("SAMPLES")]

    return samples, ground_truth, ground_prediction